/*!
copy between [`AddrConn`] and [`net::Conn`]
*/

use crate::net;
use crate::net::addr_conn::*;
use crate::net::CID;
use bytes::BytesMut;
use std::io;
use std::ops::DerefMut;
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
}

pub async fn cp_addr_conn_and_conn(args: CpAddrConnAndConnArgs) -> io::Result<u64> {
    let cid = args.cid;
    let mut ac = args.ac;
    let mut c = args.c;
    let ed_from_ac = args.ed_from_ac;
    let ed = args.ed;
    let first_target = args.first_target;
    let gtr = args.gtr;
    let no_timeout = args.no_timeout;
    use crate::Name;
    info!(cid = %cid, ac = ac.name(), "cp_addr_conn_and_conn start",);

    let tic = gtr.clone();
    scopeguard::defer! {

        if let Some(gtr) = tic {
            gtr.alive_connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        }
        info!( cid = %cid,
        "cp_addr_conn_and_conn end", );
    }
    //might discard udp addr part

    if let Some(ed) = ed {
        if ed_from_ac {
            let r = c.write(&ed).await;
            if r.is_err() {
                return r.map(|x| x as u64);
            }
        } else {
            let r = ac.w.write(&ed, &first_target.unwrap_or_default()).await;
            if r.is_err() {
                return r.map(|x| x as u64);
            }
        }
    }

    let (mut r, mut w) = tokio::io::split(c);

    if no_timeout {
        let (c1_to_c2, c2_to_c1) = (
            cp_conn_to_addr_conn(&mut r, ac.w),
            cp_addr_conn_to_conn(ac.r, &mut w),
        );

        tokio::select! {
            r1 = c1_to_c2 => {

                r1
            }
            r2 = c2_to_c1 => {

                r2
            }
        }
    } else {
        let (c1_to_c2, c2_to_c1) = (
            cp_conn_to_addr_conn(&mut r, ac.w),
            cp_addr_conn_to_conn_timeout(ac.r, &mut w),
        );

        tokio::select! {
            r1 = c1_to_c2 => {

                r1
            }
            r2 = c2_to_c1 => {

                r2
            }
        }
    }
}

pub async fn cp_conn_to_addr_conn<R>(r: &mut R, mut w: impl AddrWriteTrait) -> io::Result<u64>
where
    R: AsyncRead + Unpin + ?Sized,
{
    let mut whole: u64 = 0;
    let mut buf0 = Box::new([0u8; MTU]);

    let a = net::Addr::default();
    loop {
        let r = r.read(buf0.deref_mut()).await;
        match r {
            Ok(n) => {
                let r = w.write(&buf0[..n], &a).await;
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

    loop {
        let r1ref = &mut r;

        let sleep_f = tokio::time::sleep(CP_UDP_TIMEOUT);
        let read_f = async move {
            let mut buf0 = Box::new([0u8; MTU]);
            let mut buf = ReadBuf::new(buf0.deref_mut());
            let r = r1ref.read(buf.initialized_mut()).await;

            (r, buf0)
        };

        tokio::select! {
            _ = sleep_f =>{
                debug!("read addrconn timeout");

                break;
            }
            r = read_f =>{
                let (r,  buf0) = r;
                match r {
                    Err(_) => break,
                    Ok((m, _ad)) => {
                        if m > 0 {
                            //debug!("cp_addr_to_conn, read got {m}");
                            let r = w.write(&buf0[..m]).await;
                            if let Ok(n) = r{
                                //debug!("cp_addr_to_conn, write ok {n}");

                                whole_write += n;

                                let r = w.flush().await;
                                if r.is_err(){
                                    debug!("cp_addr_to_conn, write  flush not ok ");
                                    break;
                                }

                            }else{
                                debug!("cp_addr_to_conn, write not ok ");
                                break;
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
