/*!
copy between [`AddrConn`] and [`net::Conn`]
*/

use crate::net;
use crate::net::addr_conn::*;
use crate::net::Addr;
use crate::net::Conn;
use crate::net::CID;
use anyhow::bail;
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
use tracing::warn;

//todo: improve code

pub struct CpAddrConnArgs {
    pub cid: CID,
    pub in_conn: AddrConn,
    pub out_conn: AddrConn,
    pub ed: Option<BytesMut>,
    pub first_target: Option<Addr>,
    pub tr: Option<Arc<net::GlobalTrafficRecorder>>,
    pub no_timeout: bool,
    pub shutdown_in_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    pub shutdown_out_rx: Option<tokio::sync::oneshot::Receiver<()>>,
}

/// copy between two [`AddrConn`]
///
/// non-blocking
///
pub async fn cp_ac(args: CpAddrConnArgs) {
    use crate::Name;

    let cid = args.cid;
    let in_conn = args.in_conn;
    let mut out_conn = args.out_conn;
    let ed = args.ed;
    let first_target = args.first_target;
    let tr = args.tr;
    let no_timeout = args.no_timeout;
    let shutdown_in_rx = args.shutdown_in_rx;
    let shutdown_out_rx = args.shutdown_out_rx;

    if let Some(real_ed) = ed {
        if let Some(real_first_target) = first_target {
            debug!(cid = %cid, "cp_addr_conn: writing ed {:?}", real_ed.len());
            let r = out_conn.w.write(&real_ed, &real_first_target).await;
            if let Err(e) = r {
                warn!("cp_addr_conn: writing ed failed: {e}");
                let _ = out_conn.w.shutdown().await;
                return;
            }
        } else {
            debug!(cid = %cid,
                "cp_addr_conn: writing ed without real_first_target {:?}",
                real_ed.len()
            );
            let r = out_conn.w.write(&real_ed, &Addr::default()).await;
            if let Err(e) = r {
                warn!(cid = %cid, "cp_addr_conn: writing ed failed: {e}");
                let _ = out_conn.w.shutdown().await;
                return;
            }
        }
    }
    debug!(cid = %cid, in_c = in_conn.name(), out_c = out_conn.name(), "cp_addr_conn start",);

    tokio::spawn(net::addr_conn::cp(
        cid.clone(),
        in_conn,
        out_conn,
        tr,
        no_timeout,
        shutdown_in_rx,
        shutdown_out_rx,
    ));
}

pub struct CpAddrConnAndConnArgs {
    pub cid: CID,
    pub ac: AddrConn,
    pub c: Conn,
    pub ed_from_ac: bool,
    pub ed: Option<BytesMut>,
    pub first_target: Option<Addr>,
    pub gtr: Option<Arc<net::GlobalTrafficRecorder>>,
    pub no_timeout: bool,
    pub shutdown_ac_rx: Option<tokio::sync::oneshot::Receiver<()>>,
}

/// copy between [`AddrConn`] and [`Conn`] by calling both cp_c_to_ac and cp_ac_to_c.
///
/// blocking.
///
/// Discard udp addr part when copy from [`AddrConn`] to [`Conn`]
pub async fn cp_ac_and_c(args: CpAddrConnAndConnArgs) -> anyhow::Result<u64> {
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
            bail!("cp_addr_conn_and_conn got shutdown")
        }
        r1 = cp_c_to_ac(&mut r, ac.w) => {
            Ok(r1?)
        }
        r2 = cp_ac_to_c(ac.r, &mut w,args.no_timeout) => {
            Ok(r2?)
        }
    }
}

/// copy from [`Conn`] to [`AddrConn`], with empty addr. blocking.
pub async fn cp_c_to_ac<R>(r: &mut R, mut w: impl AddrWriteTrait) -> io::Result<u64>
where
    R: AsyncRead + Unpin + ?Sized,
{
    let mut whole: u64 = 0;
    let mut buf = Box::new([0u8; MTU]);

    let a = Addr::default();
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

/// copy from [`AddrConn`] to [`Conn`], discard addr part. blocking.
pub async fn cp_ac_to_c<W>(
    mut r: impl AddrReadTrait,
    w: &mut W,
    no_timeout: bool,
) -> io::Result<u64>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    let mut whole_write = 0;
    let mut buf = Box::new([0u8; MTU]);

    loop {
        tokio::select! {
            _ = async{
                if no_timeout{
                    std::future::pending().await
                }else{
                    tokio::time::sleep(CP_UDP_TIMEOUT).await
                }
            } =>{
                debug!("cp_addr_conn_to_conn timeout");

                break;
            }
            r =  async {
                let mut buf = ReadBuf::new(buf.deref_mut());
                r.read(buf.initialized_mut()).await
            } =>{

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
                }//match
            }//read
        } //select
    } //loop

    Ok(whole_write as u64)
}
