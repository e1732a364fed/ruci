/*!
* cp has 3 situations, pure cp, cp with gr, cp with gr and updater

the more info you want to access, the slower performance you would get
*/

use bytes::BytesMut;
use tokio::io::AsyncReadExt;

use super::*;

///   bytes increment for CID
pub type UpdateSender = tokio::sync::mpsc::Sender<(CID, u64)>;

pub type Updater = (UpdateSender, UpdateSender);
/// ub, db
pub type OptUpdater = Option<Updater>;

/// may log debug or do other side-effect stuff with id.
pub async fn copy<C1: ConnTrait, C2: ConnTrait>(
    c1: C1,
    c2: C2,
    cid: &CID,
    gtr: Option<Arc<GlobalTrafficRecorder>>,

    #[cfg(feature = "trace")] updater: OptUpdater,
) -> Result<u64, Error> {
    match gtr {
        Some(tr) => {
            cp_with_gr(
                c1,
                c2,
                cid,
                tr,
                #[cfg(feature = "trace")]
                updater,
            )
            .await
        }
        None => cp(c1, c2).await,
    }
}

/// pure copy without any side effect
pub async fn cp<C1: ConnTrait, C2: ConnTrait>(c1: C1, c2: C2) -> Result<u64, Error> {
    let (mut c1_read, mut c1_write) = tokio::io::split(c1);
    let (mut c2_read, mut c2_write) = tokio::io::split(c2);

    let (c1_to_c2, c2_to_c1) = (
        tokio::io::copy(&mut c1_read, &mut c2_write).fuse(),
        tokio::io::copy(&mut c2_read, &mut c1_write).fuse(),
    );

    pin_mut!(c1_to_c2, c2_to_c1);

    futures::select! {
        r1 = c1_to_c2 => {
            r1
        },
        r2 = c2_to_c1 => {
            r2
        },
    }
}

pub async fn cp_with_gr<C1: ConnTrait, C2: ConnTrait>(
    c1: C1,
    c2: C2,
    cid: &CID,
    tr: Arc<GlobalTrafficRecorder>,

    #[cfg(feature = "trace")] updater: OptUpdater,
) -> Result<u64, Error> {
    if log_enabled!(log::Level::Debug) {
        debug!("cp start, {} c1: {}, c2: {}", cid, c1.name(), c2.name());
    }

    let (mut c1_read, mut c1_write) = tokio::io::split(c1);
    let (mut c2_read, mut c2_write) = tokio::io::split(c2);

    #[cfg(feature = "trace")]
    match updater {
        Some(updater) => {
            let (c1_to_c2, c2_to_c1) = (
                cp_rw_with_updater(cid, &mut c1_read, &mut c2_write, updater.0).fuse(),
                cp_rw_with_updater(cid, &mut c2_read, &mut c1_write, updater.1).fuse(),
            );
            select_f(cid, c1_to_c2, c2_to_c1, tr).await
        }
        _ => {
            let (c1_to_c2, c2_to_c1) = (
                tokio::io::copy(&mut c1_read, &mut c2_write).fuse(),
                tokio::io::copy(&mut c2_read, &mut c1_write).fuse(),
            );
            select_f(cid, c1_to_c2, c2_to_c1, tr).await
        }
    }

    #[cfg(not(feature = "trace"))]
    {
        let (c1_to_c2, c2_to_c1) = (
            tokio::io::copy(&mut c1_read, &mut c2_write).fuse(),
            tokio::io::copy(&mut c2_read, &mut c1_write).fuse(),
        );
        select_f(cid, c1_to_c2, c2_to_c1, tr).await
    }
}

async fn select_f<A, B>(
    cid: &CID,
    c1_to_c2: futures::future::Fuse<A>,
    c2_to_c1: futures::future::Fuse<B>,
    tr: Arc<GlobalTrafficRecorder>,
) -> Result<u64, Error>
where
    A: futures::future::Future<Output = Result<u64, Error>>,
    B: futures::future::Future<Output = Result<u64, Error>>,
{
    pin_mut!(c1_to_c2, c2_to_c1);

    // 一个方向停止后, 关闭连接, 如果 opt 不为空, 则等待另一个方向关闭, 以获取另一方向的流量信息。

    futures::select! {
        r1 = c1_to_c2 => {

            if let Ok(n) = r1 {
                let tt = tr.ub.fetch_add(n, Ordering::Relaxed);

                if log_enabled!(log::Level::Debug) {
                    debug!("cp, {}, u, ub, {}, {}",cid,n,tt+n);
                }
            }

            // can't borrow mut more than once. We just hope tokio will shutdown tcp
            // when it's dropped.
            // during the tests we can prove it's dropped.

            let r2 = c2_to_c1.await;

            if let Ok(n) = r2 {
                let tt = tr.db.fetch_add(n, Ordering::Relaxed);

                if log_enabled!(log::Level::Debug) {
                    debug!("cp, {}, u, db, {}, {}",cid, n,tt+n);
                }
            }

            if log_enabled!(log::Level::Debug) {
                debug!("cp end u, {} ",cid);
            }

            r1
        },
        r2 = c2_to_c1 => {

            if let Ok(n) = r2 {
                let tt = tr.db.fetch_add(n, Ordering::Relaxed);

                if log_enabled!(log::Level::Debug) {
                    debug!("cp, {}, d, db, {}, {}",cid, n,tt+n);
                }
            }

            let r1 = c1_to_c2.await;

            if let Ok(n) = r1 {
                let tt = tr.ub.fetch_add(n, Ordering::Relaxed);

                if log_enabled!(log::Level::Debug) {
                    debug!("cp, {}, d, ub, {}, {}",cid,n,tt+n);
                }
            }

            if log_enabled!(log::Level::Debug) {
                debug!("cp end d, { } ",cid);
            }

            r2
        },
    }
}

//todo : improve this
async fn cp_rw_with_updater<'a, R, W>(
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
