use async_trait::async_trait;
use bytes::{Buf, BufMut, BytesMut};
use macro_map::{map_ext_fields, MapExt};
use ruci::map;
use ruci::map::*;
use ruci::net::CID;
use ruci::utils::{buf_to_ob, ob_to_buf};
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::task::{ready, Poll};
use std::{fmt::Display, pin::Pin};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{debug, info};

use crate::map::recorder::{PayloadInfo, READ_DIRECTION, WRITE_DIRECTION};

// fn direction_match_write(endpoint_type: ProxyBehavior, direction: i8) -> bool {
//     assert!(direction.abs() == 1);
//     matches!(endpoint_type, ProxyBehavior::DECODE) && direction == WRITE_DIRECTION
//         || matches!(endpoint_type, ProxyBehavior::ENCODE) && direction == READ_DIRECTION
// }

pub const WRTIE_IS_STEGO: u8 = 0;
pub const WRTIE_IS_REAL: u8 = 1;

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct Embedder {
    pub file: Arc<Vec<PayloadInfo>>,
}

impl Embedder {
    pub fn new(file_content: Vec<u8>, file_name: String) -> anyhow::Result<Self> {
        let ext = file_name
            .split('.')
            .last()
            .unwrap_or_default()
            .to_lowercase();
        let extension = ext.as_str();

        let extension = match extension {
            "json" => crate::map::recorder::OutputFileExtension::Json,
            "cbor" => crate::map::recorder::OutputFileExtension::Cbor,
            _ => anyhow::bail!("invalid file extension: {}", extension),
        };

        let info_data = crate::map::recorder::InfoData::new(file_content, extension)?;
        Ok(Self {
            file: Arc::new(info_data.payload),
            ext_fields: Default::default(),
        })
    }
}

impl Display for Embedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "embedder")
    }
}

#[async_trait]
impl Map for Embedder {
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        if matches!(behavior, ProxyBehavior::UNSPECIFIED) {
            return MapResult::from_err_str("Embedder does not support ProxyBehavior::UNSPECIFIED");
        }

        let c = params.c.try_unwrap_tcp().unwrap();

        let b = ob_to_buf(params.b);

        let (write_tx, mut write_rx) = mpsc::channel(1);
        let (read_tx, read_rx) = mpsc::channel(100);
        let (write_info_tx, write_info_rx) = mpsc::channel(1);

        let file = self.file.clone();
        let invert = matches!(behavior, ProxyBehavior::DECODE);

        let mut map_result = MapResult::default();
        map_result.a = params.a;

        match behavior {
            ProxyBehavior::UNSPECIFIED => panic!("can't happen "),
            ProxyBehavior::ENCODE => {
                if self.is_tail_of_chain() && !b.is_empty() {
                    let r = write_tx.send(b).await;
                    if let Err(e) = r {
                        return MapResult::from_e(e);
                    }
                } else {
                    map_result.b = buf_to_ob(b);
                }
            }
            ProxyBehavior::DECODE => {
                if !b.is_empty() {
                    let r = read_tx.send(b).await;
                    if let Err(e) = r {
                        return MapResult::from_e(e);
                    }
                }
            }
        }

        tokio::spawn(async move {
            let (mut r, mut w) = tokio::io::split(c);

            play_file(
                file,
                &mut r,
                &mut w,
                &mut write_rx,
                write_info_tx,
                read_tx,
                invert,
            )
            .await
        });

        let conn = EmbedConn {
            write_tx,
            read_rx,
            read_state: Default::default(),
            write_state: Default::default(),
            write_info_rx,
        };

        map_result.c = ruci::net::Stream::Conn(Box::new(conn));

        map_result
    }
}

/// 对于 本Conn， Read 是从 已有的文件中获取Info（主要是包长），
/// 然后根据 包长从 base 中读取 指定长度的信息（然后在 poll_read 中复制到buf 中），
///
/// 而 Write 是 从 base 给出的 buf 中，截取 Info 中指定的长度，写入 base.
///
/// 如果Info 中指定的长度是大于给出的 buf 的，则只能添加 padding
///
/// 这里还要看时序，如果没到该 read/write 的时机，就要等待。
pub struct EmbedConn {
    write_state: WriteState,
    read_state: ReadState,

    write_info_rx: Receiver<usize>,

    write_tx: Sender<BytesMut>,
    read_rx: Receiver<BytesMut>,
}

#[derive(Default)]
enum WriteState {
    #[default]
    Ready,
    // ContinueWrite(BytesMut),
    WriteTxPending(usize, OptDialF), //written_len
}

#[derive(Default)]
enum ReadState {
    #[default]
    Ready,
    ContinueCopyBuf(BytesMut),
    ContinueRemote(usize, Option<BytesMut>),
}
type OptDialF = Pin<
    Box<
        dyn std::future::Future<Output = Result<(), mpsc::error::SendError<BytesMut>>>
            + Send
            + Sync,
    >,
>;

impl Display for EmbedConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "embed_conn")
    }
}

impl AsyncRead for EmbedConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        rbuf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if let ReadState::ContinueCopyBuf(b) = &mut self.read_state {
                let to_put = rbuf.remaining().min(b.len());
                rbuf.put_slice(&b[..to_put]);
                b.advance(to_put);
                if b.is_empty() {
                    self.read_state = ReadState::Ready;
                }
                return Poll::Ready(Ok(()));
            }

            let r = self.read_rx.poll_recv(cx);
            match ready!(r) {
                None => return Poll::Ready(Err(io::Error::other("read_rx got none"))),

                Some(mut buf) => match &mut self.read_state {
                    ReadState::Ready => {
                        let cl = buf.get_u16() as usize;
                        if cl <= buf.len() {
                            buf.resize(cl, 0);

                            let to_put = rbuf.remaining().min(buf.len());
                            rbuf.put_slice(&buf[..to_put]);
                            buf.advance(to_put);
                            if !buf.is_empty() {
                                self.read_state = ReadState::ContinueCopyBuf(buf);
                            }
                            return Poll::Ready(Ok(()));
                        } else {
                            self.read_state = ReadState::ContinueRemote(cl, Some(buf));
                            continue;
                        }
                    }
                    ReadState::ContinueRemote(cl, olast_buf) => {
                        let cl = *cl;
                        let mut last_buf = olast_buf.take().unwrap();
                        let need = cl - last_buf.len();

                        last_buf.extend_from_slice(&buf[..need.min(buf.len())]);

                        if last_buf.len() < cl {
                            *olast_buf = Some(last_buf);
                            continue;
                        } else {
                            assert_eq!(last_buf.len(), cl);

                            let to_put = rbuf.remaining().min(cl);
                            rbuf.put_slice(&last_buf[..to_put]);
                            last_buf.advance(to_put);

                            if last_buf.is_empty() {
                                self.read_state = ReadState::Ready;
                            } else {
                                self.read_state = ReadState::ContinueCopyBuf(last_buf);
                            }

                            return Poll::Ready(Ok(()));
                        }
                    }
                    _ => panic!("should not happen"),
                },
            } //match
        } //loop
    }
}

impl AsyncWrite for EmbedConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if let WriteState::WriteTxPending(written_len, f) = &mut self.write_state {
            let r = f.as_mut().poll(cx);
            match r {
                Poll::Ready(_) => {
                    return Poll::Ready(Ok(*written_len));
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }

        let r = self.write_info_rx.poll_recv(cx);
        match ready!(r) {
            None => return Poll::Ready(Err(io::Error::other("write_info_rx got None"))),

            Some(length) => {
                match &mut self.write_state {
                    WriteState::Ready => {
                        let mut bs_to_send = BytesMut::with_capacity(length);
                        bs_to_send.put_u8(WRTIE_IS_REAL);
                        assert!(buf.len() < MAX_PACKET_LEN);
                        bs_to_send.put_u16(buf.len() as u16);

                        let written_len;

                        let left_space = length - 3;
                        if buf.len() <= left_space {
                            bs_to_send.put_slice(buf);
                            bs_to_send.resize(length, 0);

                            written_len = buf.len();
                        } else {
                            bs_to_send.put_slice(&buf[..left_space]);

                            written_len = left_space;
                        }

                        let txc = self.write_tx.clone();

                        let f = async move { txc.send(bs_to_send).await };

                        let mut f = Box::pin(f);

                        let r = f.as_mut().poll(cx);
                        match r {
                            Poll::Ready(_) => {
                                return Poll::Ready(Ok(written_len));
                            }
                            Poll::Pending => {
                                self.write_state = WriteState::WriteTxPending(written_len, f);
                                return Poll::Pending;
                            }
                        }
                    }
                    // WriteState::ContinueWrite(bytes) => {
                    //     //
                    //     todo!()
                    // }
                    _ => panic!("should not happen"),
                }
            }
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

const MAX_PACKET_LEN: usize = 64 * 1024;

/// blocking. 内部 不断地调用 read_once 和 write_once，将 读到的包去掉1字节包头后用 read_tx 发送出去。
/// 用 write_rx 接收 要写入的真实信息
pub async fn play_file<R, W>(
    file: Arc<Vec<PayloadInfo>>,
    reader: &mut R,
    writer: &mut W,
    write_rx: &mut Receiver<BytesMut>,
    write_info_tx: Sender<usize>,
    read_tx: Sender<BytesMut>,
    invert: bool,
    // shutdown_rx: tokio::sync::oneshot::Receiver<()>, //todo: allow shutdown
) where
    R: AsyncRead + Unpin + ?Sized,
    W: AsyncWrite + Unpin + ?Sized,
{
    let mut index = 0;

    let mut lst_rbuf = BytesMut::new();
    loop {
        let cur_info = file.get(index).unwrap();
        let length = cur_info.length;

        let direction = if invert {
            -cur_info.direction
        } else {
            cur_info.direction
        };
        match direction {
            WRITE_DIRECTION => {
                let r = write_info_tx.send(length).await;
                if r.is_err() {
                    break;
                }
                let r = write_once(writer, length, write_rx).await;
                if r.is_err() {
                    break;
                }
            }
            READ_DIRECTION => {
                let r = read_once(reader, length, lst_rbuf).await;
                match r {
                    Ok(mut result_buf) => {
                        assert!(result_buf.len() >= length);

                        let mut cur_read_packet = if result_buf.len() > length {
                            let real = result_buf.split_to(length);

                            lst_rbuf = result_buf;
                            real
                        } else {
                            lst_rbuf = BytesMut::new();
                            result_buf
                        };

                        let b = cur_read_packet.get_u8();

                        match b {
                            WRTIE_IS_STEGO => {}
                            WRTIE_IS_REAL => {
                                // 此时已知是 数据包了，但是还不确定是 首包还是续包，因此交给rx端处理
                                let r = read_tx.send(cur_read_packet).await;
                                if let Err(e) = r {
                                    info!("play got err {e}");
                                    break;
                                }
                            }
                            _ => panic!("can't happen"),
                        }
                    }
                    Err(_) => break,
                }
            }
            _ => panic!("can't happen"),
        }

        index += 1;
        if index == file.len() {
            index = 0;
        }
    }
}

/// blocking. read_once 的作用是， 保证读到一个完整的符合长度的包. 若返回的 包长于 length, 则说明粘包了，下一次调用
/// read_once 时，要传入 减去 length 的 剩余部分
async fn read_once<R>(r: &mut R, length: usize, mut last_buf: BytesMut) -> io::Result<BytesMut>
where
    R: AsyncRead + Unpin + ?Sized,
{
    let mut read_start_index;

    if last_buf.is_empty() {
        read_start_index = 0;
    } else {
        read_start_index = last_buf.len();
    }
    last_buf.resize(MAX_PACKET_LEN, 0);

    let mut buf = last_buf;

    use tokio::io::AsyncReadExt;
    loop {
        let result = r.read(&mut buf[read_start_index..]).await;
        match result {
            Ok(n) => {
                if n == 0 {
                    info!("read_once got EOF");
                    return Err(std::io::Error::other("EOF"));
                }
                let whole_read_len = read_start_index + n;

                if whole_read_len >= length {
                    return Ok(buf);
                } else {
                    read_start_index += n;
                    continue;
                }
            }
            Err(e) => return Err(e),
        }
    }
}

///blocking. write_once 检查包长 而是 直接发送。 使包满足 packet长度 以及包头等情况 都是 调用者的责任
///
/// 如果100ms后收不到write_rx 中的数据，则会自动发送一个隐写包
async fn write_once<W>(
    writer: &mut W,
    length: usize,
    write_rx: &mut Receiver<BytesMut>,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    let timer = tokio::time::sleep(std::time::Duration::from_millis(100));
    use tokio::io::AsyncWriteExt;

    tokio::select! {
        _ = timer=>{
            let mut v = vec![WRTIE_IS_STEGO];
            v.resize(length, 0);
           return writer.write_all(&v).await;
        }
        op = write_rx.recv() =>{
            match op{
                Some(v) => {

                    let r = writer.write_all(&v).await;
                    match r {
                        Ok(_) => return Ok(()),
                        Err(e) => {
                            info!("mpsc_transmit, write got e: {e}");
                            return Err(e);
                        },
                    }
                },
                None => {
                    debug!("mpsc_transmit write got none, will shutdown");
                   let _ =  writer.write(&[]).await;
                    let _ = writer.shutdown().await;
                    return Ok(())
                },
            }

        }

    }
}

// pub enum WritePacket {
//     Stego(usize),
//     Real(Vec<u8>),
// }
/*
fn transmit(
    c: ruci::net::Conn,
    capacity: usize,
) -> (
    Sender<WritePacket>, //写入时可选两种数据.  注意，这里的real 数据是已经加好包头的，故不用再在这里处理
    Receiver<Vec<u8>>, //接收所有数据（因为有可能是粘包等情况，因此不能直接丢掉）
) {
    let (read_tx, read_rx) = channel::<Vec<u8>>(capacity);
    let (write_tx, mut write_rx) = channel::<WritePacket>(capacity);

    let (mut r, mut w) = tokio::io::split(c);
    use tokio::io::AsyncWriteExt;

    tokio::spawn(async move {
        let mut zbuf = BytesMut::zeroed(1024 * 8);
        zbuf[0] = WRTIE_IS_STEGO;

        loop {
            tokio::select! {
                o_wbuf = write_rx.recv() =>{
                    match o_wbuf {
                        Some(v) => {
                            let v = match &v{
                                WritePacket::Real(vec) => vec,

                                WritePacket::Stego(n) => {
                                    if *n > zbuf.len(){
                                       zbuf.resize(*n,0)
                                    }

                                    &zbuf[..*n]

                                },
                            };
                            let r = w.write_all(&v).await;
                            match r {
                                Ok(_) => continue,
                                Err(e) => {
                                    info!("mpsc_transmit, write got e: {e}");
                                    break;
                                },
                            }
                        },
                        None => {
                            debug!("mpsc_transmit write got none, will shutdown");
                           let _ =  w.write(&[]).await;
                            let _ = w.shutdown().await;
                        },
                    }
                }
            }
        }
    });

    use tokio::io::AsyncReadExt;
    tokio::spawn(async move {
        let mut buf = BytesMut::zeroed(1024 * 8); // 这里假设 buf 足够大

        loop {
            tokio::select! {
                read_result = r.read(&mut buf)=>{
                    match read_result {
                        Ok(n) => {

                            let v = buf[..n].to_vec();
                           let r =  read_tx.send(v).await;

                           if let Err(e) = r{
                                info!("mpsc_transmit, read send got e: {e}");
                                break;
                           }

                        },
                        Err(e) => {
                            info!("mpsc_transmit, read got e: {e}");
                            break;
                        },
                    }

                }
            }
        }
    });

    (write_tx, read_rx)
}

 */
