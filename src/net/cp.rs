/*!
*/

use super::*;
use bytes::BytesMut;
use tokio::io::AsyncReadExt;

///   bytes increment for CID
pub type UpdateSender = tokio::sync::mpsc::Sender<(CID, u64)>;

/// ub, db
pub type Updater = (UpdateSender, UpdateSender);
pub type OptUpdater = Option<Updater>;

#[allow(unused)]
pub async fn copy<C1: AsyncConn, C2: AsyncConn>(
    local_c: &mut C1,
    remote_c: &mut C2,
    cid: &CID,

    #[cfg(feature = "trace")] updater: OptUpdater,
) -> Result<(u64, u64), Error> {
    #[cfg(feature = "trace")]
    {
        cp_with_updater(local_c, remote_c, cid, updater).await
    }

    #[cfg(not(feature = "trace"))]
    tokio::io::copy_bidirectional(local_c, remote_c).await
}

/// cp with updater will send msg when each single read/write ends.
///
/// Note: the more info you want to access, the slower performance you would get
#[cfg(feature = "trace")]
pub async fn cp_with_updater<C1: AsyncConn, C2: AsyncConn>(
    c1: &mut C1,
    c2: &mut C2,
    cid: &CID,

    updater: OptUpdater,
) -> Result<(u64, u64), Error> {
    use futures::FutureExt;

    match updater {
        Some(updater) => {
            let (mut c1_read, mut c1_write) = tokio::io::split(c1);
            let (mut c2_read, mut c2_write) = tokio::io::split(c2);

            let (c1_to_c2, c2_to_c1) = (
                cp_rw_with_updater(cid, &mut c1_read, &mut c2_write, updater.0).fuse(),
                cp_rw_with_updater(cid, &mut c2_read, &mut c1_write, updater.1).fuse(),
            );
            select_f(cid, c1_to_c2, c2_to_c1).await
        }
        _ => tokio::io::copy_bidirectional(c1, c2).await,
    }
}

#[cfg(feature = "trace")]
async fn select_f<A, B>(
    cid: &CID,
    c1_to_c2: futures::future::Fuse<A>,
    c2_to_c1: futures::future::Fuse<B>,
) -> Result<(u64, u64), Error>
where
    A: futures::future::Future<Output = Result<u64, Error>>,
    B: futures::future::Future<Output = Result<u64, Error>>,
{
    futures_util::pin_mut!(c1_to_c2, c2_to_c1);

    // 一个方向停止后, 关闭连接, 如果 opt 不为空, 则等待另一个方向关闭, 以获取另一方向的流量信息.

    use tracing::trace;

    futures::select! {
        r1 = c1_to_c2 => {

            let mut n1 :u64 = 0;

            if let Ok(n) = r1 {
                n1 = n;

                if tracing::enabled!(tracing::Level::TRACE)  {
                    trace!(cid = %cid,"cp, u, ub, {}, {}",n, n);
                }
            }

            // can't borrow mut more than once. We just hope tokio will shutdown tcp
            // when it's dropped.
            // during the tests we can prove it's dropped.

            let mut n2 :u64 = 0;

            let r2 = c2_to_c1.await;

            if let Ok(n) = r2 {
                n2 = n;


                if tracing::enabled!(tracing::Level::TRACE)  {
                    trace!(cid = %cid,"cp, u, db, {}, {}", n, n);
                }
            }

            if tracing::enabled!(tracing::Level::TRACE)  {
                trace!(cid = %cid,"cp end u");
            }

            Ok((n1,n2))
        },
        r2 = c2_to_c1 => {
            let mut n1 :u64 = 0;

            if let Ok(n) = r2 {
                n1 = n;


                if tracing::enabled!(tracing::Level::TRACE)  {
                    trace!(cid = %cid,"cp, d, db, {}, {}", n, n);
                }
            }

            let r1 = c1_to_c2.await;
            let mut n2 :u64 = 0;

            if let Ok(n) = r1 {
                n2 = n;


                if tracing::enabled!(tracing::Level::TRACE)  {
                    trace!(cid = %cid,"cp, d, ub, {}, {}",n, n);
                }
            }

            if tracing::enabled!(tracing::Level::TRACE)  {
                trace!(cid = %cid,"cp end d");
            }

            Ok((n1,n2))
        },
    }
}

//todo : improve this
pub async fn cp_rw_with_updater<'a, R, W>(
    cid: &CID,
    reader: &'a mut R,
    writer: &'a mut W,
    us: UpdateSender,
) -> std::io::Result<u64>
where
    R: AsyncRead + Unpin + ?Sized,
    W: AsyncWrite + Unpin + ?Sized,
{
    let mut buf = BytesMut::zeroed(8192);
    let mut total: u64 = 0;
    loop {
        let r = reader.read(&mut buf).await;
        match r {
            Ok(n) => {
                if n == 0 {
                    return Ok(total);
                }
                let r = writer.write_all(&buf[..n]).await;
                match r {
                    Ok(_) => {
                        let n64 = n as u64;
                        total += n64;
                        let _ = us.send((cid.clone(), n64)).await;
                    }
                    Err(e) => {
                        if total != 0 {
                            return Ok(total);
                        }
                        return Err(e);
                    }
                }
            }
            Err(e) => {
                if total != 0 {
                    return Ok(total);
                }
                return Err(e);
            }
        }
    }
}
