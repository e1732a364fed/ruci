use ::http::HeaderValue;
use anyhow::{bail, Context};
use async_trait::async_trait;
use bytes::BytesMut;
use futures::Future;
use macro_mapper::NoMapperExt;
use ruci::{
    map::{self, *},
    net::{self, http::CommonConfig, *},
};
use tokio_tungstenite::{
    client_async,
    tungstenite::http::{Request, StatusCode},
};

use super::*;

#[derive(Clone, Debug, Default, NoMapperExt)]
pub struct Client {
    request: Request<()>,
    use_early_data: bool,
}

impl ruci::Name for Client {
    fn name(&self) -> &str {
        "websocket_client"
    }
}

impl Client {
    pub fn new(c: CommonConfig) -> Self {
        let mut request = Request::builder()
            .method("GET")
            .header("Host", c.host.as_str())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .uri("ws://".to_string() + c.host.as_str() + &c.path);

        if let Some(h) = c.headers {
            for (k, v) in h.iter() {
                if k != "Host" {
                    request = request.header(k.as_str(), v.as_str());
                }
            }
        }

        let r = request.body(()).unwrap();
        Self {
            request: r,
            use_early_data: c.use_early_data.unwrap_or_default(),
        }
    }

    async fn get_conn_by_req(
        b: Option<BytesMut>,
        mut req: Request<()>,
        conn: net::Conn,
    ) -> anyhow::Result<net::Conn> {
        if let Some(ref b) = b {
            //debug!("ws client will use earlydata {}", b.len());
            use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

            let str = URL_SAFE_NO_PAD.encode(b);
            req.headers_mut().insert(
                EARLY_DATA_HEADER_KEY,
                HeaderValue::from_str(&str).expect("ok"),
            );
        }

        let (c, resp) = client_async(req, conn)
            .await
            .with_context(|| "websocket client handshake failed")?;

        if resp.status() != StatusCode::SWITCHING_PROTOCOLS {
            bail!(
                "websocket client handshake got resp status not SWITCHING_PROTOCOLS: {}",
                resp.status()
            );
        }

        Ok(Box::new(WsStreamToConnWrapper {
            ws: Box::pin(c),
            r_buf: None,
            w_buf: None,
        }))
    }

    async fn handshake(
        &self,
        conn: net::Conn,
        a: Option<net::Addr>,
        b: Option<BytesMut>,
    ) -> anyhow::Result<map::MapResult> {
        let req = self.request.clone();
        if self.use_early_data {
            return Ok(MapResult::new_c(Box::new(EarlyConn {
                request: req,
                base_c: Some(conn),
                ..Default::default()
            }))
            .a(a)
            .b(b)
            .build());
        }

        let c = Client::get_conn_by_req(None, req, conn).await?;

        Ok(MapResult::new_c(c).a(a).b(b).build())
    }
}

#[async_trait]
impl Mapper for Client {
    async fn maps(
        &self,
        _cid: CID,
        _behavior: ProxyBehavior,
        params: map::MapParams,
    ) -> map::MapResult {
        let conn = params.c;
        if let Stream::Conn(conn) = conn {
            let r = self.handshake(conn, params.a, params.b).await;
            match r {
                anyhow::Result::Ok(r) => r,
                Err(e) => MapResult::from_e(e.context("websocket_client maps failed")),
            }
        } else {
            MapResult::err_str("websocket_client only support tcplike stream")
        }
    }
}

type OptDialF = Option<Pin<Box<dyn Future<Output = anyhow::Result<Conn>> + Send + Sync>>>;

#[derive(Default)]
struct EarlyConn {
    request: Request<()>,
    real_c: Option<Pin<net::Conn>>,
    base_c: Option<net::Conn>,

    dial_f: OptDialF,
    first_data_len: usize,
    left_first_w_data: Option<BytesMut>,
}

impl ruci::Name for EarlyConn {
    fn name(&self) -> &str {
        "websocket_ed_conn"
    }
}

impl AsyncRead for EarlyConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.real_c {
            Some(c) => c.as_mut().poll_read(cx, buf),
            None => Poll::Ready(Err(io_error("can't poll_read when not established"))),
        }
    }
}

impl AsyncWrite for EarlyConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        loop {
            match &mut self.as_mut().dial_f {
                Some(f) => {
                    return match ready!(f.as_mut().poll(cx)) {
                        Ok(c) => {
                            let mut pinc = Box::pin(c);
                            self.dial_f = None;
                            if let Some(mut left_w_data) = self.left_first_w_data.take() {
                                let r2 = pinc.as_mut().poll_write(cx, &left_w_data);
                                self.real_c = Some(pinc);

                                match r2 {
                                    Poll::Ready(r) => match r {
                                        Ok(u) => {
                                            if u < left_w_data.len() {
                                                left_w_data.advance(u);
                                                self.left_first_w_data = Some(left_w_data);
                                            }
                                            return Poll::Ready(Ok(self.first_data_len + u));
                                        }
                                        Err(e) => return Poll::Ready(Err(e)),
                                    },
                                    Poll::Pending => {
                                        self.left_first_w_data = Some(left_w_data);
                                        return Poll::Pending;
                                    }
                                }
                            } else {
                                self.real_c = Some(pinc);

                                Poll::Ready(Ok(self.first_data_len))
                            }
                        }
                        Err(e) => Poll::Ready(Err(io_error(e))),
                    };
                }
                None => {
                    if self.real_c.is_some() {
                        if let Some(mut left_w_data) = self.left_first_w_data.take() {
                            left_w_data.extend_from_slice(buf);

                            let len = left_w_data.len();

                            let r2 = self
                                .real_c
                                .as_mut()
                                .expect("ok")
                                .as_mut()
                                .poll_write(cx, &left_w_data);

                            match r2 {
                                Poll::Ready(r) => match r {
                                    Ok(u) => {
                                        if u < len {
                                            left_w_data.advance(u);
                                            self.left_first_w_data = Some(left_w_data);
                                        }
                                        return Poll::Ready(Ok(u));
                                    }
                                    Err(e) => return Poll::Ready(Err(e)),
                                },
                                Poll::Pending => {
                                    self.left_first_w_data = Some(left_w_data);
                                    return Poll::Pending;
                                }
                            }
                        } else {
                            return self
                                .real_c
                                .as_mut()
                                .expect("ok")
                                .as_mut()
                                .poll_write(cx, buf);
                        }
                    } else {
                        let mut bl = buf.len();
                        if bl > MAX_EARLY_DATA_LEN {
                            bl = MAX_EARLY_DATA_LEN;
                            self.left_first_w_data =
                                Some(BytesMut::from(&buf[MAX_EARLY_DATA_LEN..]))
                        }
                        self.first_data_len = bl;

                        let f = Client::get_conn_by_req(
                            Some(BytesMut::from(&buf[..bl])),
                            std::mem::take(&mut self.request),
                            self.base_c.take().expect("base_c ok"),
                        );

                        // Must store the future in struct, can't poll here directly using ready! .
                        // As when got pending, as the function returns, the future
                        // will be dropped, resulting the dropping of the base conn,
                        // resulting the disconnection

                        self.dial_f = Some(Box::pin(f));
                    }
                }
            }
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context,
    ) -> Poll<Result<(), io::Error>> {
        match &mut self.real_c {
            Some(c) => c.as_mut().poll_flush(cx),
            None => Poll::Ready(Err(io_error("can't flush when not established"))),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context,
    ) -> Poll<Result<(), io::Error>> {
        match &mut self.real_c {
            Some(c) => c.as_mut().poll_shutdown(cx),
            None => Poll::Ready(Err(io_error("can't poll_shutdown when not established"))),
        }
    }
}
