/*!
Defines a Map that uses lua code as its maps method.

In order to let lua take full use of rust code, we have to wrap everything for lua.
 */

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::task::Poll;

use async_trait::async_trait;
use bytes::BufMut;
use bytes::BytesMut;
use macro_map::{map_ext_fields, MapExt};
use mlua::prelude::*;
use mlua::BString;
use mlua::UserData;
use mlua::UserDataMethods;
use mlua::Value;
use ruci::map::MapBox;
use ruci::net::Addr;
use ruci::{
    map::{self, Map, MapResult},
    net::{Stream, CID},
    Name,
};
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tracing::debug;

/// 被用于 infinite.rs 中 给 lua 添加 Create_out_map 和 Create_in_map 函数.
#[derive(Clone)]
pub struct MapWrapper(pub Arc<MapBox>);

impl FromLua for MapWrapper {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.take::<Self>()?),
            _ => unreachable!(),
        }
    }
}
impl UserData for MapWrapper {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("clone", |_, m, ()| Ok(m.clone()));
    }
}

/// bool 为 true 表示 is None
pub struct AddrWrapper(pub Addr, pub bool);

impl AddrWrapper {
    pub fn to_opt_addr(self) -> Option<Addr> {
        match self.1 {
            true => None,
            false => Some(self.0),
        }
    }
}

impl UserData for AddrWrapper {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("network", |_, a, ()| Ok(a.0.network.to_string()));
        methods.add_method("get_addr_str", |_, a, ()| Ok(a.0.get_addr_str()));
        methods.add_method("is_none", |_, a, ()| Ok(a.1));
    }
}

/// bool 为 true 表示 is None
pub struct BytesMutWrapper(pub BytesMut, pub bool);

impl BytesMutWrapper {
    pub fn to_opt_bytesmut(self) -> Option<BytesMut> {
        match self.1 {
            true => None,
            false => Some(self.0),
        }
    }
}

impl UserData for BytesMutWrapper {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("is_none", |_, a, ()| Ok(a.1));

        methods.add_method("len", |_, b, ()| Ok(b.0.len()));
        methods.add_method("to_string", |lua, b, ()| Ok(lua.create_string(&b.0)));

        methods.add_method_mut("put_u16", |_, b, u| {
            b.0.put_u16(u);
            Ok(())
        });
    }
}

/// 将 ruci::net::Conn 包装 并提供给 lua 使用
pub struct RustConn(pub ruci::net::Conn);

impl UserData for RustConn {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        //https://github.com/mlua-rs/mlua/blob/main/examples/async_tcp_server.rs

        methods.add_async_method_mut("read", |lua, mut this, size| async move {
            let mut buf = vec![0; size];
            debug!("reading...");
            let n = this.0.read(&mut buf).await?;
            debug!("read returned");

            buf.truncate(n);

            // lua 的 string 直接就可以包含任意二进制数据
            lua.create_string(&buf)
        });

        methods.add_async_method_mut("write", |_, mut this, data: mlua::BString| async move {
            let n = this.0.write(&data).await?;
            Ok(n)
        });

        methods.add_async_method_mut("close", |_, mut this, ()| async move {
            this.0.shutdown().await?;
            Ok(())
        });

        methods.add_async_method_mut("flush", |_, mut this, ()| async move {
            this.0.flush().await?;
            // debug!("flush ok");
            Ok(())
        });
    }
}

pub struct LuaConn {
    lua: Lua,
    read_key: String,
    write_key: String,
    close_key: String,
    flush_key: String,

    read_future: OptReadF,
    write_future: OptWriteF,
}

type OptReadF =
    Option<std::pin::Pin<Box<dyn Future<Output = Result<LuaString, LuaError>> + Send + Sync>>>;

type OptWriteF =
    Option<std::pin::Pin<Box<dyn Future<Output = Result<usize, LuaError>> + Send + Sync>>>;

impl AsyncRead for LuaConn {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut future = match self.read_future.take() {
            Some(f) => f,
            None => {
                let read_f: LuaFunction = self.lua.globals().get(self.read_key.as_str()).unwrap();
                let max_len = buf.capacity() - buf.filled().len();

                let future = read_f.call_async(max_len);

                Box::pin(future)
            }
        };

        let pr: Poll<Result<LuaString, LuaError>> = future.as_mut().poll(cx);

        match pr {
            Poll::Ready(r) => match r {
                Ok(s) => {
                    let bs = s.as_bytes();

                    buf.put_slice(&bs);

                    Poll::Ready(Ok(()))
                }
                Err(e) => Poll::Ready(Err(io::Error::other(e))),
            },
            Poll::Pending => {
                debug!("read got pending");

                let _ = self.read_future.insert(future);

                Poll::Pending
            }
        }
    }
}

impl AsyncWrite for LuaConn {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let mut future = match self.write_future.take() {
            Some(f) => f,
            None => {
                let wf: LuaFunction = self.lua.globals().get(self.write_key.as_str()).unwrap();

                let future = wf.call_async(BString::from(buf));

                Box::pin(future)
            }
        };

        let pr: Poll<Result<usize, LuaError>> = future.as_mut().poll(cx);

        match pr {
            Poll::Ready(r) => {
                debug!("write ready {r:?}");
                match r {
                    Ok(n) => Poll::Ready(Ok(n)),
                    Err(e) => Poll::Ready(Err(io::Error::other(e))),
                }
            }
            Poll::Pending => {
                debug!("write pending");
                let _ = self.write_future.insert(Box::pin(future));

                Poll::Pending
            }
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let ff: LuaFunction = self.lua.globals().get(self.flush_key.as_str()).unwrap();

        let f = ff.call_async(());

        let pr: Poll<Result<(), LuaError>> = Future::poll(std::pin::pin!(f), cx);

        match pr {
            Poll::Ready(r) => {
                // debug!("flush ready {r:?}");

                match r {
                    Ok(_) => Poll::Ready(Ok(())),
                    Err(e) => Poll::Ready(Err(io::Error::other(e))),
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        debug!("shutdown called");
        let close_f: LuaFunction = self.lua.globals().get(self.close_key.as_str()).unwrap();

        let f = close_f.call_async(());

        let pr: Poll<Result<(), LuaError>> = Future::poll(std::pin::pin!(f), cx);

        match pr {
            Poll::Ready(r) => match r {
                Ok(_) => Poll::Ready(Ok(())),
                Err(e) => Poll::Ready(Err(io::Error::other(e))),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

/// 与 在 lua中调用 Create_in_map 来将 lua配置导入 rust 代码 来生成 LuaMapWrapper 不同,
/// LuaMap 是在 rust 中直接调用 lua 代码 来生成一个 Map
#[map_ext_fields]
#[derive(Debug, Clone, MapExt, Default)]
pub struct LuaMap {
    pub lua_text: String,        //整个 lua文件的内容
    pub handshake_f_key: String, //lua文件中 对应的 map 函数的 函数名
}

impl Name for LuaMap {
    fn name(&self) -> &'static str {
        "lua_map"
    }
}

#[async_trait]
impl Map for LuaMap {
    async fn maps(
        &self,
        cid: CID,
        behavior: map::ProxyBehavior,
        params: map::MapParams,
    ) -> MapResult {
        // 每一个 maps 调用都要使用全新的 Lua State，因为 Lua本身不支持真正的多线程，
        // 但 maps却是 多线程 调用的，同一时间可能有很多个 maps 调用

        let lua = Lua::new();
        let _: () = lua
            .load(self.lua_text.clone())
            .eval()
            .context("eval lua failed")
            .unwrap();
        let handshake_f: LuaFunction = lua.globals().get(self.handshake_f_key.as_str()).unwrap();

        match params.c {
            Stream::Conn(c) => {
                let a = match params.a {
                    Some(a) => AddrWrapper(a, false),
                    None => AddrWrapper(Addr::default(), true),
                };

                let conn_v = RustConn(c);

                let b = match params.b {
                    Some(b) => BytesMutWrapper(b, false),
                    None => BytesMutWrapper(BytesMut::new(), true),
                };

                let cid_v = lua.to_value(&cid).ok().unwrap();

                let bi: usize = behavior.into(); // 将 behavior enum 传成数字

                // 返回  {read, write, close, flush} ,其中为函数名，然后后面代码再将其包装为 一个 LuaConn
                // 如果 不为 Table 而为一个 UserData, 则 其为 传入的 RustConn
                let r = handshake_f.call::<(Value, Value, Value)>((cid_v, bi, a, b, conn_v));
                match r {
                    Ok(r) => {
                        let c: Box<dyn ruci::net::AsyncConn> = match r.0 {
                            LuaValue::Table(table) => {
                                let read_key: String = table.get(1).unwrap();
                                let write_key: String = table.get(2).unwrap();
                                let close_key: String = table.get(3).unwrap();
                                let flush_key: String = table.get(4).unwrap();

                                let nc = LuaConn {
                                    lua,
                                    read_key,
                                    write_key,
                                    close_key,
                                    flush_key,
                                    read_future: None,
                                    write_future: None,
                                };
                                Box::new(nc)
                            }

                            LuaValue::UserData(any_user_data) => {
                                let b: RustConn = any_user_data.take().unwrap();
                                b.0
                            }

                            _ => todo!(),
                        };

                        //a
                        let a = match r.1 {
                            LuaNil => None,
                            LuaValue::String(s) => {
                                let s = s.to_string_lossy();
                                if s.is_empty() {
                                    None
                                } else {
                                    Addr::from_network_addr_url(&s).ok()
                                }
                            }
                            LuaValue::UserData(any_user_data) => {
                                let a: AddrWrapper = any_user_data.take().unwrap();
                                a.to_opt_addr()
                            }
                            LuaValue::Error(_error) => todo!(),
                            _ => todo!(),
                        };

                        //b
                        let b = match r.2 {
                            LuaNil => None,
                            LuaValue::String(ls) => {
                                let bs: &[u8] = &ls.as_bytes();
                                Some(BytesMut::from(bs))
                            }
                            LuaValue::Table(_table) => todo!(),
                            LuaValue::UserData(any_user_data) => {
                                let b: BytesMutWrapper = any_user_data.take().unwrap();
                                b.to_opt_bytesmut()
                            }
                            LuaValue::Error(_error) => todo!(),
                            _ => todo!(),
                        };

                        MapResult::new_c(c).a(a).b(b).build()
                    }
                    Err(e) => MapResult::from_e(e),
                }
            }
            _ => MapResult::err_str("LuaMap only support tcplike stream"),
        }
    }
}
