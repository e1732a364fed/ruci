use crate::net;
use crate::net::addr_conn::*;
use crate::net::Addr;
use crate::net::CID;
use bytes::BytesMut;
use futures_util::pin_mut;
use futures_util::select;
use futures_util::FutureExt;
use log::debug;
use log::info;
use std::io;
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::io::ReadBuf;

//todo: improve code

pub async fn cp_udp_tcp(
    cid: CID,
    mut ac: net::addr_conn::AddrConn,
    mut c: net::Conn,
    ed_from_ac: bool,
    ed: Option<BytesMut>,
    ti: Option<Arc<net::TransmissionInfo>>,
) -> io::Result<u64> {
    info!("{}, relay udp to tcp start", cid);

    let tic = ti.clone();
    scopeguard::defer! {

        if let Some(ti) = tic {
            ti.alive_connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        }
        info!("{},udp to tcp relay end", cid);
    }
    //discard udp addr part or use Addr::default

    if let Some(ed) = ed {
        if ed_from_ac {
            let r = c.write(&ed).await;
            if r.is_err() {
                return r.map(|x| x as u64);
            }
        } else {
            let r = ac.1.write(&ed, &Addr::default()).await;
            if r.is_err() {
                return r.map(|x| x as u64);
            }
        }
    }

    let (mut r, mut w) = tokio::io::split(c);
    let (c1_to_c2, c2_to_c1) = (
        cp_conn_to_addr(&mut r, ac.1).fuse(),
        cp_addr_to_conn(ac.0, &mut w).fuse(),
    );
    pin_mut!(c1_to_c2, c2_to_c1);

    select! {
        r1 = c1_to_c2 => {

            r1
        }
        r2 = c2_to_c1 => {

            r2
        }
    }
}

pub async fn cp_conn_to_addr<R, W1: AddrWriteTrait>(r1: &mut R, mut w1: W1) -> io::Result<u64>
where
    R: AsyncRead + Unpin + ?Sized,
{
    let mut whole: u64 = 0;
    let mut buf0 = Box::new([0u8; MAX_DATAGRAM_SIZE]);

    let a = net::Addr::default();
    loop {
        let r = r1.read(buf0.deref_mut()).await;
        match r {
            Ok(n) => {
                let r = w1.write(&mut buf0[..n], &a).await;
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

pub async fn cp_addr_to_conn<W, R1: AddrReadTrait>(mut r1: R1, w1: &mut W) -> io::Result<u64>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    let mut whole_write = 0;

    loop {
        let r1ref = &mut r1;

        let sleepf = tokio::time::sleep(CP_UDP_TIMEOUT).fuse();
        let readf = async move {
            let mut buf0 = Box::new([0u8; MAX_DATAGRAM_SIZE]);
            let mut buf = ReadBuf::new(buf0.deref_mut());
            let r = r1ref.read(buf.initialized_mut()).await;

            (r, buf0)
        }
        .fuse();
        pin_mut!(sleepf, readf);

        select! {
            _ = sleepf =>{
                debug!("read addrconn timeout");

                break;
            }
            r = readf =>{
                let (r,  buf0) = r;
                match r {
                    Err(_) => break,
                    Ok((m, _ad)) => {
                        if m > 0 {

                            let r = w1.write(&buf0[..m]).await;
                            if let Ok(n) = r{
                                whole_write += n;

                            }

                        }
                    }
                }
            }
        } //select
    } //loop

    Ok(whole_write as u64)
}
