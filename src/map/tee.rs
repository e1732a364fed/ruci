/*!
Implements a [`Map`] that returns a [`StreamGenerator`] which split the original stream into 3 streams.


## 关于

tokio 中类似的功能的工具是 tokio_utils::io::InspectReader 和 inspectWriter

不过, 它是接受一个 闭包, 每次读或写数据时, 都调用那个 闭包
<https://github.com/tokio-rs/tokio/issues/4584>

<https://docs.rs/tokio-util/latest/tokio_util/io/struct.InspectReader.html>

（而 golang 中的 TeeReader 是 接受一个 Writer, 每次读数据时，都写入那个 Writer

二者效果是完全一样的）

我们在这里是想要使用 chain 的 多流发生器的原理, 将一个 流变成 3个流: 一个 主流 和
两个只读流, 其中一个只读流是读 主流 write 的数据, 一个 只读流是 读 主流 read 的数据.

## 举例

内部直接用 tokio 的办法.
每 从 tls read 到数据, 就调用 tee的 f1, 每 向 tls write 数据, 就调用 tee 的 f2

设一个 chain 为 tcp->tls , 加tee后, 变 tcp->tls->tee

之后 tee 返回一个 StreamGenerator, 有 3 个 stream, 如 stream1, stream2 和 stream3

其中, 第一个 也就是 stream1 是主流. stream1 每 write 一次, 就会写入实际的 tls, 同时调用f2

然后 对 stream2 读 就能读到 stream1 所写的东西。

tls 每向 stream1 write 一次, 即 stream1 每 read 到一次, 就会同时调用 f1,

然后 对 stream3 读就能读到 stream1 所 read到的东西。

stream2 和 stream3 都是 只读的.

因为分了主流和分流, 每种流对应的后面的chain 是不一样的, 所以 这涉及到配置


*/

use super::*;

use crate::map;
use crate::{net::*, Name};
use async_trait::async_trait;

use futures::executor::block_on;
use macro_map::*;
use tokio::sync::mpsc;

/// split the incomming stream into 3 sub streams: one main rw stream,
/// two readonly stream.
#[map_ext_fields]
#[derive(Debug, Clone, Default, MapExt)]
pub struct Tee {}

impl Name for Tee {
    fn name(&self) -> &'static str {
        "tee"
    }
}

#[async_trait]
impl Map for Tee {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            Stream::Conn(c) => {
                let (r, w) = tokio::io::split(c);

                let (r_tx, r_rx) = mpsc::channel(300); //todo: adjust this
                let (w_tx, w_rx) = mpsc::channel(300);

                let r = tokio_util::io::InspectReader::new(r, move |buf| {
                    block_on(async {
                        let b = BytesMut::from(buf);
                        let _ = r_tx.send(b).await;
                    })
                });
                let w = tokio_util::io::InspectWriter::new(w, move |buf| {
                    block_on(async {
                        let b = BytesMut::from(buf);
                        let _ = w_tx.send(b).await;
                    })
                });

                let rc = helpers::MpscRWrapper { r: r_rx };
                let wc = helpers::MpscRWrapper { r: w_rx };

                let (generator_tx, generator_rx) = mpsc::channel(3);

                tokio::spawn(async move {
                    let rw = helpers::RWWrapper { r, w };

                    let nid = cid.clone_push_num(1);
                    let r1 = MapResult::new_c(Box::new(rw)).new_id(nid).build();

                    let nid = cid.clone_push_num(2);
                    let r2 = MapResult::new_c(Box::new(rc)).new_id(nid).build();

                    let nid = cid.clone_push_num(3);
                    let r3 = MapResult::new_c(Box::new(wc)).new_id(nid).build();

                    let _ = generator_tx.send(r1).await;
                    let _ = generator_tx.send(r2).await;
                    let _ = generator_tx.send(r3).await;
                });

                return MapResult::builder()
                    .a(params.a)
                    .b(params.b)
                    .c(net::Stream::Generator(generator_rx))
                    .build();
            }
            _ => {
                return MapResult::err_str(&format!(
                    "tee only support Conn stream, got {}",
                    params.c
                ))
            }
        }
    }
}
