/*!
Defines a steganography protocol example1 "spe1".

使用 html, 用 post 表示 client 向 server 上传数据，

称一对问答为一个QA，128个QA为一个Token。2个Token可表示1bit信息。

server 收到后，以 对应的 answer 回答，之后 提供client 一串新问题， 声称表示"相关问题"，
实际为 server 向 client 发送的数据。
client 不回答这些相关问题，而是继续 post.

不过，如果上传数据或者下载数据太长的话，就会成为明显的特征，因此如果数据比较长，需要进行
随机截断。

因此本协议对于一个tcp长连接，会依次产生多个 短连接。

因此在服务端，需要方法来判断一个请求到底是一个新请求还是某个旧请求的下一部分。
因此在中断处，会以一个中断标识来注明。若一个连接最后没有中断标识，那就是结束。

因此需要在首部进行标识。首部以32个Token(=16bit=2字节)表示本次请求的id;
每个id在一段时间内（如1小时内）只能用一次. 该id的连接结束后，同一客户端上的下一个id为上一个
id+1; 如此便可区别不同的客户端 以及 不同的请求连接。

同一id重新出现一次时，若之前接到过同id的中断标识，即表示这是该连接的下一个片段。而若没接到，则
说明该请求可能不来自我们的隐写协议，此时依然以与原来相同的方式反回 答案，以及随机的 "相关问题"。


目前暂未实现分段
*/

use std::io::Result;
use std::task::ready;
use std::{
    collections::HashMap,
    fmt::Write,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::bail;
use bytes::BytesMut;
use itertools::Itertools;
use macro_map::*;
use rand::Rng;
use ruci::net::helpers::{
    ContentLenProtocolBufReadResult, ContentLenProtocolBufReader, ContentLenProtocolPacketMetadata,
};
use ruci::net::http::CommonHttp;
use ruci::net::Addr;
use ruci::{
    map::{self, MapParams, MapResult, ProxyBehavior},
    net::CID,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tracing::{trace, warn};
// use tracing::trace;

pub type QA = (String, String);
pub type Token = [QA; 128]; //每个 Token都有128种可能, 128个问答同时表示同一种信息

/// QaData 为 本隐写示例所使用的相关数据
///
/// 选择的方式按 转移方阵中所指定的概率来
#[derive(Debug)]
pub struct QaData {
    //对应 0 和 1 的 问答 组合 各 128 个
    pub qa_set: [Token; 2],

    // 用于快速用String 查找对应 Question
    q_hash_map: [HashMap<String, u8>; 2],
    // 用于快速用String 查找对应 Answer
    a_hash_map: [HashMap<String, u8>; 2],

    // 首部id标识
    // head_marker_set: [Token; 32],

    // 中断标识
    // interrupt_set: Token,

    //所有的 QA的转移方阵，其维度为 qa_set中的 Vec的 长度.每行总和均为1
    //也可以不提供，不提供则所问的问题完全随机化
    pub transformation_matrix: Option<[Vec<Vec<f32>>; 2]>,
}

fn simple_qa(from: usize, plus: usize) -> QA {
    let n = from as u16 + plus as u16;
    (format!("question {n}"), format!("answer {n}"))
}

fn simple_token_vec(count: usize) -> Vec<Token> {
    let mut r = vec![];
    for i in 0..count {
        let from = i * 128;
        let array: Token = array_init::array_init(|ii| simple_qa(from, ii));
        r.push(array);
    }

    r
}

type BytesAndIndexVec = (BytesMut, Option<Vec<(bool, u8)>>);

impl QaData {
    pub fn new(qa_set: [Token; 2]) -> QaData {
        let q_hash_map: [HashMap<String, u8>; 2] = array_init::array_init(|i| {
            qa_set[i]
                .iter()
                .enumerate()
                .map(|(index, qa)| (qa.0.clone(), index as u8))
                .collect::<HashMap<String, u8>>()
        });

        let a_hash_map: [HashMap<String, u8>; 2] = array_init::array_init(|i| {
            qa_set[i]
                .iter()
                .enumerate()
                .map(|(index, qa)| (qa.1.clone(), index as u8))
                .collect::<HashMap<String, u8>>()
        });
        QaData {
            qa_set,
            q_hash_map,
            a_hash_map,
            // head_marker_set: <[Token; 32]>::try_from(simple_token_vec(32))
            //     .expect("Conversion failed"),
            // interrupt_set: simple_token(384),
            transformation_matrix: None,
        }
    }
    pub fn new_simple() -> QaData {
        let qa_set = <[Token; 2]>::try_from(simple_token_vec(2)).expect("Conversion failed");

        Self::new(qa_set)
    }

    /// 由给定的 question-answer 对 的Vec 来初始化 QaData.
    /// qas.len() 须为偶数。
    /// 若 qas.len() > 256, 则会truncate.
    /// 若 qas.len() < 256, 则会truncate到2的幂后分一半 duplicate 对应次数；
    ///
    /// 前一半的问答代表0， 后一半 代表1.
    /// 约定 每个question-answer的权重都是相等的，即问任一问题
    /// 的机会相等。
    ///
    pub fn from(mut qas: Vec<QA>) -> QaData {
        let len = qas.len();

        let qa_set = {
            match len.cmp(&256) {
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => {
                    qas.truncate(256);
                }
                std::cmp::Ordering::Less => {
                    let target_n = nearest_power_of_two(len as u32) as usize;

                    qas.truncate(target_n);

                    let half = target_n / 2;

                    let mut first_part = qas.split_off(half);
                    let mut second_part = qas;

                    for _ in 1..128 / half {
                        first_part.extend_from_within(0..half);
                        second_part.extend_from_within(0..half);
                    }

                    assert_eq!(first_part.len(), second_part.len());
                    assert_eq!(first_part.len(), 128);

                    first_part.append(&mut second_part);

                    qas = first_part;
                }
            };
            let first_part = qas.split_off(128);
            let second_part = qas;

            [
                <Token>::try_from(first_part).expect("array from vec same size is ok"),
                <Token>::try_from(second_part).expect("array from vec same size is ok"),
            ]
        };

        Self::new(qa_set)
    }

    // select question randomly
    pub fn bytes_to_questions_text(&self, buf: &[u8]) -> String {
        let mut rng = rand::thread_rng();

        let mut s = String::new();

        buf.iter().for_each(|byte| {
            for i in 0..8 {
                let mask = 1 << (7 - i); // 从最高位到最低位
                let mut bit = byte & mask; // 检查位是否为 1
                if bit > 0 {
                    bit = 1;
                }

                let number = rng.gen_range(0..128);

                let qa = &self.qa_set[bit as usize][number];
                s.push_str(qa.0.as_str());
                s.push('\n');
            }
        });
        s.pop();
        s
    }

    // u8 表示 问题索引
    //true 代表1，false 代表0，错误代表 其不在 本QA表中。
    pub fn match_question(&self, s: &str) -> anyhow::Result<(bool, u8)> {
        let o = self.q_hash_map[0].get(s);
        match o {
            Some(i) => Ok((false, *i)),
            None => {
                let o = self.q_hash_map[1].get(s);
                match o {
                    Some(i) => Ok((true, *i)),
                    None => bail!("match_question err: there is no question, string is: {s}"),
                }
            }
        }
    }

    pub fn match_answer(&self, s: &str) -> anyhow::Result<(bool, u8)> {
        let o = self.a_hash_map[0].get(s);
        match o {
            Some(i) => Ok((false, *i)),
            None => {
                let o = self.a_hash_map[1].get(s);
                match o {
                    Some(i) => Ok((true, *i)),
                    None => bail!("there is no answer: {s}"),
                }
            }
        }
    }

    // 包含转换后的 数据 以及对应的 问题索引
    pub fn questions_to_bytes(
        &self,
        str: &str,
        with_answer_index: bool,
        has_header: bool,
    ) -> anyhow::Result<BytesAndIndexVec> {
        let lines: Vec<&str> = str.split('\n').collect();

        let mut bools: Vec<bool> = vec![];
        let mut ai: Vec<(bool, u8)> = vec![];

        let mut dealed_first = false;
        if !has_header {
            dealed_first = true;
        }

        for l in lines {
            if !dealed_first {
                if l != "Questions:" {
                    bail!("questions_to_bytes: no 'Questions:' header");
                }
                dealed_first = true;
            } else {
                match self.match_question(l) {
                    Ok(r) => {
                        bools.push(r.0);
                        if with_answer_index {
                            ai.push(r);
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        let bytes = bools_to_bytes(bools);

        let bm = BytesMut::from(bytes.as_slice());

        match with_answer_index {
            true => Ok((bm, Some(ai))),
            false => Ok((bm, None)),
        }
    }

    pub fn answers_to_bytes(&self, str: &str) -> anyhow::Result<BytesMut> {
        let lines: Vec<&str> = str.split('\n').collect();

        let mut bools: Vec<bool> = vec![];

        for l in lines {
            match self.match_answer(l) {
                Ok(r) => {
                    bools.push(r.0);
                }
                Err(e) => return Err(e),
            }
        }

        Ok(BytesMut::from(bools_to_bytes(bools).as_slice()))
    }

    /// return content_len (without header len)
    pub fn write_client_data(&self, data: &[u8], write_cache: &mut BytesMut) -> usize {
        let mut body_buf = BytesMut::new();
        let _ = body_buf.write_str("Questions:\n");
        let s = self.bytes_to_questions_text(data);
        body_buf.extend_from_slice(s.as_bytes());

        write_cache.clear();

        let content_len = body_buf.len();

        let _ = write_cache.write_str(
            "POST /ask HTTP/1.1\r\nHost: httpbin.org\r\nConnection: keep-alive\r\nContent-Length: ",
        );
        let _ = write_cache.write_str(&content_len.to_string());
        let _ = write_cache.write_str("\r\nContent-Type: text/plain\r\n\r\n");

        write_cache.extend_from_slice(&body_buf);
        content_len
    }

    pub fn write_server_data(
        &self,
        data: &[u8],
        write_cache: &mut BytesMut,
        server_cached_answers: &[(bool, u8)],
    ) -> usize {
        let mut content_buf = BytesMut::with_capacity(READ_CAP);
        let s = server_cached_answers
            .iter()
            .map(|pair| match pair.0 {
                true => self.qa_set[1][pair.1 as usize].1.as_str(),
                false => self.qa_set[0][pair.1 as usize].1.as_str(),
            })
            .join("\n");

        content_buf.extend_from_slice(s.as_bytes());

        let _ = content_buf.write_str(SERVER_QUESTION_PROMPT);

        let s = self.bytes_to_questions_text(data);
        content_buf.extend_from_slice(s.as_bytes());

        let content_len = content_buf.len();

        write_cache.clear();
        let _ =write_cache.write_str("http/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Type: text/plain\r\nContent-Length: ");
        let _ = write_cache.write_str(&content_len.to_string());
        let _ = write_cache.write_str("\r\n\r\n");

        write_cache.extend_from_slice(&content_buf);
        content_len
    }
}

fn nearest_power_of_two(mut n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    n |= n >> 1;
    n |= n >> 2;
    n |= n >> 4;
    n |= n >> 8;
    n |= n >> 16;
    n - (n >> 1) // 返回小于或等于 n 的最大 2 的幂
}

// 大端序，即 vec![true] 会被转成 vec![128]
fn bools_to_bytes(bools: Vec<bool>) -> Vec<u8> {
    // bools
    //     .chunks(8) // 每 8 个布尔值分成一组
    //     .map(|chunk| {
    //         chunk.iter().enumerate().fold(0u8, |byte, (i, &b)| {
    //             if b {
    //                 byte | (1 << (7 - i)) // 将对应的比特位设置为 1
    //             } else {
    //                 byte // 不改变比特位
    //             }
    //         })
    //     })
    //     .collect()

    let mut result = Vec::new();
    let mut byte = 0u8;
    for (i, &b) in bools.iter().enumerate() {
        if b {
            byte |= 1 << (7 - (i % 8));
        }
        if i % 8 == 7 || i == bools.len() - 1 {
            result.push(byte);
            byte = 0;
        }
    }
    result
}

#[test]
fn q2t() {
    let x = QaData::new_simple();
    println!(
        "{}",
        x.bytes_to_questions_text(&[1, 4, 3, 2, 5, 7, 8, 33, 241])
    )
}

#[test]
fn b2b() {
    let x = vec![];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![false];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, false];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, false, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true, true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true, true, true, true, true];
    println!("{:?}", bools_to_bytes(x));

    let x = vec![
        true, false, true, true, false, false, true, false, false, true, true, false, false, true,
        false, true,
    ];
    println!("{:?}", bools_to_bytes(x));
}

#[derive(Default)]
pub enum WriteState {
    #[default]
    ReadyForNew,
    Previous(usize),
}

/// Represents a steganographic connection that hides data in HTTP Q&A pairs
///
/// This implementation uses a question-answer based protocol where:
/// - Each QA pair represents 1 bit of information
/// - 128 QA pairs form a Token
/// - 2 Tokens can represent 1 bit of actual data
pub struct Conn {
    // 将相关字段组织在一起
    // 连接信息
    cid: CID,
    is_server: bool,

    // 协议相关
    qa: Arc<QaData>,
    server_cached_answers: Vec<(bool, u8)>,

    // IO 相关
    base_w: Pin<Box<dyn AsyncWrite + Send + Sync>>,
    write_cache: Option<BytesMut>,
    write_state: WriteState,
    reader: ContentLenProtocolBufReader,
}

pub const READ_CAP: usize = 1024 * 1024;
const SERVER_QUESTION_PROMPT: &str = "\nYou can also ask these questions:\n";

fn get_pr_header_content_length_body_index(
    pr: Box<dyn CommonHttp>,
) -> Result<ContentLenProtocolPacketMetadata> {
    match pr.get_header("Content-Length") {
        Some(h) => {
            let clr: usize = h.value.parse().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("content length 无法解析为整数 ,{e}"),
                )
            })?;
            Ok(ContentLenProtocolPacketMetadata {
                content_len: clr,
                body_start_index: pr.get_body_start_index(),
            })
        }
        None => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "no content length",
        )),
    }
}

impl Conn {
    /// Parses steganographic string data into actual bytes
    ///
    /// # Arguments
    /// * `from` - Start index in the buffer
    /// * `to` - End index in the buffer
    /// * `buf` - Target buffer to write parsed data
    /// * `data` - Raw input data containing steganographic content
    fn real_read(
        &mut self,
        from: usize,
        to: usize,
        buf: &mut ReadBuf<'_>,
        data: &[u8],
    ) -> Poll<Result<()>> {
        let real_data = &data[from..to];
        let real_string = String::from_utf8_lossy(real_data).into_owned();
        trace!( cid=%self.cid,"spe1 real_read called ");

        if self.is_server {
            Poll::Ready(match self.qa.questions_to_bytes(&real_string, true, true) {
                Ok(bm) => {
                    trace!(cid=%self.cid, "spe1 real_read {}, {}",bm.0.len(),&data[..to].len() );

                    buf.put_slice(&bm.0);
                    self.server_cached_answers = bm.1.unwrap();

                    Ok(())
                }
                Err(e) => Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("spe1 real_read: question string to bytes err: {e}"),
                )),
            })
        } else {
            match Self::server_response_to_2_parts(&real_string) {
                None => {
                    warn!("spe1 real_read, server_response_to_2_parts failed");
                    Poll::Ready(Ok(()))
                }

                Some(answers_questions) => {
                    //舍弃 part1, 只使用 part2

                    Poll::Ready(
                        match self
                            .qa
                            .questions_to_bytes(answers_questions.1, false, false)
                        {
                            Ok(bm) => {
                                buf.put_slice(&bm.0);
                                trace!( cid=%self.cid,"spe1 real_read put_slice {} ",bm.0.len());

                                Ok(())
                            }
                            Err(e) => Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("spe1 real_read: questions string to bytes err: {e}"),
                            )),
                        },
                    )
                }
            }
        }
    }

    fn server_response_to_2_parts(s: &str) -> Option<(&str, &str)> {
        s.split_once(SERVER_QUESTION_PROMPT)
            .map(|parts| (parts.0, parts.1))
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        rbuf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        match ready!(self.reader.read(cx)) {
            Ok(Some(ContentLenProtocolBufReadResult {
                buf,
                body_from: from,
                body_to: to,
            })) => {
                debug_assert!(from < to);

                let r = self.real_read(from, to, rbuf, &buf);
                self.reader.put_back(buf);
                r
            }
            Ok(None) => Poll::Ready(Ok(())),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}
impl AsyncWrite for Conn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        if self.reader.is_closed() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "spe1: writing when read end closed",
            )));
        }

        match self.write_state {
            WriteState::ReadyForNew => {
                let mut write_cache = self.write_cache.take().unwrap();

                if write_cache.capacity() < READ_CAP {
                    write_cache.resize(READ_CAP, 0);
                }
                write_cache.clear();

                let content_len = if self.is_server {
                    self.qa
                        .write_server_data(buf, &mut write_cache, &self.server_cached_answers)
                } else {
                    self.qa.write_client_data(buf, &mut write_cache)
                };

                trace!(
                    "spe1 ready will write with content_len {}, buf.len {}",
                    content_len,
                    buf.len()
                );

                let wl = write_cache.len();

                match self.base_w.as_mut().poll_write(cx, &write_cache) {
                    Poll::Pending => {
                        let _ = self.write_cache.insert(write_cache);

                        Poll::Pending
                    }

                    Poll::Ready(r) => match r {
                        Err(e) => {
                            let _ = self.write_cache.insert(write_cache);

                            Poll::Ready(Err(e))
                        }

                        Ok(n) => match n.cmp(&wl) {
                            std::cmp::Ordering::Less => {
                                trace!( cid=%self.cid,
                                    "spe1 partial write {n}, {}, {}",
                                    wl, buf.len()
                                );

                                self.write_state = WriteState::Previous(n);
                                let _ = self.write_cache.insert(write_cache);

                                self.poll_write(cx, buf)
                            }
                            std::cmp::Ordering::Equal => {
                                let _ = self.write_cache.insert(write_cache);

                                trace!( cid=%self.cid,"spe1  write ok {n}, {}",buf.len());

                                Poll::Ready(Ok(buf.len()))
                            }
                            std::cmp::Ordering::Greater => {
                                let cl = write_cache.len();
                                let _ = self.write_cache.insert(write_cache);

                                Poll::Ready(Err(io::Error::other(format!(
                                    "write len not right, {n}, {cl}",
                                ))))
                            }
                        },
                    },
                }
            }
            WriteState::Previous(head) => {
                let write_cache = self.write_cache.take().unwrap();

                let write_data = &write_cache[head..];

                match self.base_w.as_mut().poll_write(cx, write_data) {
                    Poll::Pending => {
                        let _ = self.write_cache.insert(write_cache);

                        trace!( cid=%self.cid,
                            "spe1 partial write2  continue got pending, {}",buf.len(),
                        );
                        Poll::Pending
                    }

                    Poll::Ready(r) => match r {
                        Err(e) => {
                            self.write_state = WriteState::ReadyForNew;
                            let _ = self.write_cache.insert(write_cache);

                            Poll::Ready(Err(e))
                        }
                        Ok(n) => match n.cmp(&write_data.len()) {
                            std::cmp::Ordering::Less => {
                                trace!( cid=%self.cid,
                                    "spe1 partial write2 {head} {n}, {}, {}",
                                    write_cache.len(),buf.len()
                                );

                                let _ = self.write_cache.insert(write_cache);

                                self.write_state = WriteState::Previous(head + n);

                                self.poll_write(cx, buf)
                            }
                            std::cmp::Ordering::Equal => {
                                let _ = self.write_cache.insert(write_cache);
                                self.write_state = WriteState::ReadyForNew;

                                trace!( cid=%self.cid,"spe1 partial write2 finish, {}",buf.len());

                                Poll::Ready(Ok(buf.len()))
                            }
                            std::cmp::Ordering::Greater => {
                                let cl = write_cache.len();
                                let _ = self.write_cache.insert(write_cache);
                                self.write_state = WriteState::ReadyForNew;

                                Poll::Ready(Err(io::Error::other(format!(
                                    "write len not right, {n}, {cl}",
                                ))))
                            }
                        },
                    },
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.base_w.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.base_w.as_mut().poll_shutdown(cx)
    }
}

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct ClientOrServer {
    pub qa: Arc<QaData>,
    pub is_server: bool,
}

// impl ruci::Name for ClientOrServer {
//     fn name(&self) -> &'static str {
//         "spe1"
//     }
// }

impl ClientOrServer {
    fn connect_with_rw(
        &self,
        r: Box<dyn AsyncRead + Send + Sync + 'static + Unpin>,
        w: Box<dyn AsyncWrite + Send + Sync + 'static + Unpin>,
        a: Option<Addr>,
        b: Option<BytesMut>,
        cid: CID,
    ) -> MapResult {
        let is_ser = self.is_server;

        let content_len_body_start_index_parse_fn = move |data: &[u8]| {
            let pr = ruci::net::http::common_parse(is_ser, data);
            get_pr_header_content_length_body_index(pr)
        };
        let c = Conn {
            cid,
            qa: self.qa.clone(),
            is_server: self.is_server,
            server_cached_answers: vec![],
            base_w: Box::pin(w),
            write_cache: Some(BytesMut::with_capacity(READ_CAP)),
            write_state: WriteState::default(),
            reader: ContentLenProtocolBufReader::new(
                READ_CAP,
                Box::pin(r),
                Box::new(content_len_body_start_index_parse_fn),
            ),
        };

        MapResult::new_c(Box::new(c)).a(a).b(b).build()
    }
}

#[async_trait::async_trait]
impl map::Map for ClientOrServer {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            ruci::net::Stream::RW((r, w)) => self.connect_with_rw(r, w, params.a, params.b, cid),
            ruci::net::Stream::Conn(base) => {
                let (r, w) = tokio::io::split(base);

                self.connect_with_rw(Box::new(r), Box::new(w), params.a, params.b, cid)
            }
            _ => MapResult::from_err_str("spe1 only support tcplike stream"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // QaData 相关测试
    mod qa_data_tests {
        use super::*;

        #[test]
        fn test_new_simple() {
            let qa = QaData::new_simple();
            assert_eq!(qa.qa_set.len(), 2);
            assert_eq!(qa.qa_set[0].len(), 128);
            assert_eq!(qa.qa_set[1].len(), 128);
        }

        #[test]
        fn test_from_vec() {
            // 测试正常大小的输入
            let qas = vec![
                ("q1".to_string(), "a1".to_string()),
                ("q2".to_string(), "a2".to_string()),
                ("q3".to_string(), "a3".to_string()),
                ("q4".to_string(), "a4".to_string()),
            ];
            let qa = QaData::from(qas.clone());

            // 验证数据被正确复制填充到128个条目
            assert_eq!(qa.qa_set[0].len(), 128);
            assert_eq!(qa.qa_set[1].len(), 128);

            // 验证原始数据被正确保留
            let first_two = &qa.qa_set[0][0..2];
            assert_eq!(first_two[0], qas[0]);
            assert_eq!(first_two[1], qas[1]);
        }

        #[test]
        fn test_oversized_input() {
            // 创建超过256个QA对的输入
            let mut qas = Vec::new();
            for i in 0..300 {
                qas.push((format!("question_{}", i), format!("answer_{}", i)));
            }

            let qa = QaData::from(qas);

            // 验证被截断到256个条目
            assert_eq!(qa.qa_set[0].len(), 128);
            assert_eq!(qa.qa_set[1].len(), 128);
        }

        #[test]
        fn test_bytes_to_questions() {
            let qa = QaData::new_simple();

            // 测试空输入
            assert_eq!(qa.bytes_to_questions_text(&[]), "");

            // 测试单字节
            let result = qa.bytes_to_questions_text(&[0x55]); // 0x55 = 0b01010101
            let questions: Vec<&str> = result.split('\n').collect();
            assert_eq!(questions.len(), 8); // 一个字节应产生8个问题

            // 测试多字节
            let result = qa.bytes_to_questions_text(&[0xFF, 0x00]);
            let questions: Vec<&str> = result.split('\n').collect();
            assert_eq!(questions.len(), 16); // 两个字节应产生16个问题
        }

        #[test]
        fn test_question_matching() {
            let qa = QaData::new_simple();

            // 测试有效问题
            let valid_q = &qa.qa_set[0][0].0;
            let result = qa.match_question(valid_q);
            assert!(result.is_ok());

            // 测试无效问题
            let result = qa.match_question("invalid_question");
            assert!(result.is_err());

            // 测试0和1位的问题都能正确匹配
            let q0 = &qa.qa_set[0][0].0;
            let q1 = &qa.qa_set[1][0].0;

            let (bit0, _) = qa.match_question(q0).unwrap();
            let (bit1, _) = qa.match_question(q1).unwrap();

            assert!(!bit0); // 0位问题应返回false
            assert!(bit1); // 1位问题应返回true
        }
    }

    // 编解码相关测试
    mod encoding_tests {
        use super::*;

        #[test]
        fn test_bools_to_bytes() {
            let ev: Vec<bool> = vec![];
            let eb: Vec<u8> = vec![];
            // 测试空输入
            assert_eq!(bools_to_bytes(ev), eb);

            // 测试单个比特
            assert_eq!(bools_to_bytes(vec![true]), vec![128]);
            assert_eq!(bools_to_bytes(vec![false]), vec![0]);

            // 测试完整字节
            assert_eq!(
                bools_to_bytes(vec![true, true, true, true, true, true, true, true]),
                vec![255]
            );

            // 测试部分字节
            assert_eq!(
                bools_to_bytes(vec![true, false, true, false]),
                vec![160] // 0b10100000
            );

            // 测试多个字节
            assert_eq!(
                bools_to_bytes(vec![
                    true, true, true, true, false, false, false, false, false, false, false, false,
                    true, true, true, true
                ]),
                vec![0xF0, 0x0F]
            );
        }
    }

    // 连接相关测试
    mod connection_tests {
        use map::Map;
        use parking_lot::Mutex;
        use ruci::net;
        use tokio::io::AsyncReadExt;

        use super::*;

        #[tokio::test]
        async fn test_client_server_communication() -> anyhow::Result<()> {
            let qa_data = Arc::new(QaData::new_simple());

            // 创建客户端和服务器
            let client = ClientOrServer {
                qa: qa_data.clone(),
                is_server: false,
                ext_fields: Some(map::MapExtFields::default()),
            };

            let server = ClientOrServer {
                qa: qa_data.clone(),
                is_server: true,
                ext_fields: Some(map::MapExtFields::default()),
            };

            // 测试数据
            let test_data = b"Hello, World!";

            // 模拟客户端发送数据
            let mut client_write = BytesMut::new();
            let content_len = client.qa.write_client_data(test_data, &mut client_write);
            assert!(content_len > 0);

            // 验证服务器能正确解码数据
            let writev = Arc::new(Mutex::new(Vec::new()));
            let mock_tcp = net::helpers::mock::MockTcpStream {
                read_data: client_write.to_vec(),
                write_data: Vec::new(),
                write_target: Some(writev.clone()),
            };

            let server_conn = server
                .maps(
                    CID::default(),
                    ProxyBehavior::DECODE,
                    MapParams::new(Box::new(mock_tcp)),
                )
                .await;

            let mut read_buf = [0u8; 1024];
            let n = server_conn
                .c
                .try_unwrap_tcp()?
                .read(&mut read_buf[..])
                .await?;

            assert_eq!(&read_buf[..n], test_data);

            Ok(())
        }

        #[tokio::test]
        async fn test_connection_error_handling() -> anyhow::Result<()> {
            let qa_data = Arc::new(QaData::new_simple());

            let client = ClientOrServer {
                qa: qa_data,
                is_server: false,
                ext_fields: Some(map::MapExtFields::default()),
            };

            // 测试无效数据
            let invalid_data = b"Invalid HTTP Data";
            let writev = Arc::new(Mutex::new(Vec::new()));
            let mock_tcp = net::helpers::mock::MockTcpStream {
                read_data: invalid_data.to_vec(),
                write_data: Vec::new(),
                write_target: Some(writev),
            };

            let result = client
                .maps(
                    CID::default(),
                    ProxyBehavior::DECODE,
                    MapParams::new(Box::new(mock_tcp)),
                )
                .await;

            // 验证错误处理
            assert!(result.e.is_none());

            let mut read_buf = [0u8; 1024];
            let r = result.c.try_unwrap_tcp()?.read(&mut read_buf[..]).await;

            assert!(r.is_err());
            Ok(())
        }
    }
}
