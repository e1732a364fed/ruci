/*!
copy between [`AddrConn`] and [`net::Conn`]
*/

use crate::net;
use crate::net::addr_conn::*;
use crate::net::CID;
use crate::utils::io_error;
use bytes::BytesMut;
use std::io;
use std::ops::DerefMut;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::io::ReadBuf;
use tracing::debug;
use tracing::info;

//todo: improve code

pub struct CpAddrConnAndConnArgs {
    pub cid: CID,
    pub ac: net::addr_conn::AddrConn,
    pub c: net::Conn,
    pub ed_from_ac: bool,
    pub ed: Option<BytesMut>,
    pub first_target: Option<net::Addr>,
    pub gtr: Option<Arc<net::GlobalTrafficRecorder>>,
    pub no_timeout: bool,
    pub shutdown_ac_rx: Option<tokio::sync::oneshot::Receiver<()>>,
}

/// blocking. discard udp addr part when copy from AddrConn to Conn
pub async fn cp_addr_conn_and_conn(args: CpAddrConnAndConnArgs) -> io::Result<u64> {
    let cid = args.cid;
    let mut ac = args.ac;
    let mut c = args.c;
    let ed_from_ac = args.ed_from_ac;
    let gtr = args.gtr;
    use crate::Name;
    info!(cid = %cid, ac = ac.name(), "cp_addr_conn_and_conn start",);

    if let Some(ed) = args.ed {
        if ed_from_ac {
            c.write_all(&ed).await?;
        } else {
            ac.w.write(&ed, &args.first_target.unwrap_or_default())
                .await?;
        }
    }

    if let Some(gtr) = &gtr {
        gtr.alive_connection_count.fetch_add(1, Ordering::Relaxed);
    }

    scopeguard::defer! {

        if let Some(gtr) = &gtr {
            gtr.alive_connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        }
        info!( cid = %cid,
        "cp_addr_conn_and_conn end", );
    }

    let (mut r, mut w) = tokio::io::split(c);

    tokio::select! {
        _ = async{
            if let Some(shut) = args.shutdown_ac_rx{
                shut.await
            }else{
                std::future::pending().await
            }
        }=>{
            Err(io_error("got shutdown"))
        }
        r1 = cp_conn_to_addr_conn(&mut r, ac.w) => {
            r1
        }
        r2 = async{
            if args.no_timeout{
                cp_addr_conn_to_conn(ac.r, &mut w).await
            }else{
                cp_addr_conn_to_conn_timeout(ac.r, &mut w).await
            }
        } => {
            r2
        }
    }
}

pub async fn cp_conn_to_addr_conn<R>(r: &mut R, mut w: impl AddrWriteTrait) -> io::Result<u64>
where
    R: AsyncRead + Unpin + ?Sized,
{
    let mut whole: u64 = 0;
    let mut buf = Box::new([0u8; MTU]);

    let a = net::Addr::default();
    loop {
        let r = r.read(buf.deref_mut()).await;
        match r {
            Ok(n) => {
                let r = w.write(&buf[..n], &a).await;
                match r {
                    Ok(n) => whole += n as u64,
                    Err(_) => break,
                }
            }
            Err(_) => {
                break;
            }
        }
    }

    Ok(whole)
}

pub async fn cp_addr_conn_to_conn_timeout<W>(
    mut r: impl AddrReadTrait,
    w: &mut W,
) -> io::Result<u64>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    let mut whole_write = 0;
    let mut buf = Box::new([0u8; MTU]);

    loop {
        let r_ref = &mut r;
        let buf_ref = buf.deref_mut();

        let read_f = async move {
            let mut buf = ReadBuf::new(buf_ref);
            r_ref.read(buf.initialized_mut()).await
        };

        tokio::select! {
            _ = tokio::time::sleep(CP_UDP_TIMEOUT) =>{
                debug!("cp_addr_conn_to_conn timeout");

                break;
            }
            r = read_f =>{

                match r {
                    Err(e) => {
                        debug!("cp_addr_to_conn, read got e, will break: {e}");
                        break
                    },
                    Ok((m, _ad)) => {
                        if m > 0 {
                            //debug!("cp_addr_to_conn, read got {m}");
                            let r = w.write(&buf[..m]).await;
                            match r{
                                Ok(n) => {
                                    //debug!("cp_addr_to_conn, write ok {n}");

                                    whole_write += n;

                                    let r = w.flush().await;
                                    if let Err(e) = r{
                                        debug!("cp_addr_to_conn, write  flush not ok: {e}");
                                        break;
                                    }
                                },
                                Err(_) => {
                                    debug!("cp_addr_to_conn, write not ok ");
                                    break;
                                },
                            }
                        }
                    }
                }
            }
        } //select
    } //loop

    Ok(whole_write as u64)
}

pub async fn cp_addr_conn_to_conn<W>(mut r: impl AddrReadTrait, w: &mut W) -> io::Result<u64>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    let mut whole_write = 0;

    loop {
        let rr = &mut r;

        let mut buf0 = Box::new([0u8; MTU]);
        let mut buf = ReadBuf::new(buf0.deref_mut());
        let r = rr.read(buf.initialized_mut()).await;

        match r {
            Err(e) => {
                debug!("cp_addr_to_conn, read not ok {e}");

                break;
            }
            Ok((m, _ad)) => {
                if m > 0 {
                    //debug!("cp_addr_to_conn, read got {m}");
                    let r = w.write(&buf0[..m]).await;
                    if let Ok(n) = r {
                        //debug!("cp_addr_to_conn, write ok {n}");

                        whole_write += n;

                        let r = w.flush().await;
                        if r.is_err() {
                            debug!("cp_addr_to_conn, write  flush not ok {r:?}");
                            break;
                        }
                    } else {
                        debug!("cp_addr_to_conn, write not ok {r:?}");
                        break;
                    }
                }
            }
        }
    } //loop

    Ok(whole_write as u64)
}
