/*!
Defines a Map that uses lua code as its maps method.

In order to let lua take full use of rust code, we have to wrap everything for lua.
 */

use std::future::Future;
use std::io;
use std::os::raw::c_void;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
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
use tokio::io::ReadBuf;

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

        methods.add_method_mut("put_slice", |_, b, s: mlua::BString| {
            b.0.put_slice(&s);
            Ok(())
        });
    }
}

pub struct WritePollResult(pub Poll<std::io::Result<usize>>);

impl UserData for WritePollResult {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method_mut("is_pending", |_, this, ()| async move {
            Ok(this.0.is_pending())
        });

        methods.add_async_method_mut("is_err", |_, this, ()| async move {
            Ok(match &this.0 {
                Poll::Ready(r) => r.is_err(),
                Poll::Pending => false,
            })
        });

        methods.add_async_method_mut("get_n", |_, this, ()| async move {
            Ok(match &this.0 {
                Poll::Ready(r) => match r {
                    Ok(n) => *n,
                    Err(_) => 0,
                },
                Poll::Pending => 0,
            })
        });
    }
}

pub struct EmptyPollResult(pub Poll<io::Result<()>>);
impl UserData for EmptyPollResult {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method_mut("is_pending", |_, this, ()| async move {
            Ok(this.0.is_pending())
        });

        methods.add_async_method_mut("is_err", |_, this, ()| async move {
            Ok(match &this.0 {
                Poll::Ready(r) => r.is_err(),
                Poll::Pending => false,
            })
        });
    }
}

pub struct ReadBufWrapper {
    pub real_buf: Option<BytesMut>,
    pub ptr: LuaLightUserData,
}

impl ReadBufWrapper {
    pub fn new(n: usize) -> Self {
        let mut bs = BytesMut::zeroed(n);
        let rb = Box::new(ReadBuf::new(&mut bs));

        let raw_ptr: *mut c_void = Box::into_raw(rb) as *mut c_void;

        ReadBufWrapper {
            real_buf: Some(bs),
            ptr: LuaLightUserData(raw_ptr),
        }
    }

    pub fn release(&mut self) {
        let buf_void = self.ptr.0;
        assert!(!buf_void.is_null());

        let rb = buf_void as *mut ReadBuf<'_>;

        unsafe {
            let _ = Box::from_raw(rb);
        };
        self.real_buf = None;
        self.ptr = LuaLightUserData(std::ptr::null_mut())
    }
}

impl UserData for ReadBufWrapper {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("get_ptr", |_lua, this, ()| Ok(this.ptr));

        methods.add_method_mut("drop", |_lua, this, ()| {
            this.release();
            Ok(())
        });

        methods.add_method_mut("remaining", |_, this, ()| {
            let void = this.ptr.0;
            assert!(!void.is_null());

            let rb = unsafe { &mut *(void as *mut ReadBuf<'_>) };

            Ok(rb.remaining())
        });

        methods.add_method_mut("filled_len", |_, this, ()| {
            let void = this.ptr.0;
            assert!(!void.is_null());

            let rb = unsafe { &mut *(void as *mut ReadBuf<'_>) };

            Ok(rb.filled().len())
        });

        methods.add_method_mut("filled_content", |lua, this, n: usize| {
            let void = this.ptr.0;
            assert!(!void.is_null());

            let rb = unsafe { &mut *(void as *mut ReadBuf<'_>) };

            lua.create_string(&rb.filled()[..n])
        });

        methods.add_method_mut("put_slice", |_lua, this, data: mlua::BString| {
            let buf_void = this.ptr.0;
            assert!(!buf_void.is_null());

            let rb = unsafe { &mut *(buf_void as *mut ReadBuf<'_>) };

            rb.put_slice(data.as_slice());
            Ok(())
        });
    }
}

/// 将 ruci::net::Conn 包装 并提供给 lua 使用
pub struct RustConn {
    pub conn: Pin<ruci::net::Conn>,
}

impl UserData for RustConn {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        //https://github.com/mlua-rs/mlua/blob/main/examples/async_tcp_server.rs

        methods.add_async_method_mut("read", |lua, mut this, size| async move {
            let mut buf = vec![0; size];
            // debug!("reading...");
            let n = this.conn.read(&mut buf).await?;
            // debug!("read returned");

            buf.truncate(n);

            // lua 的 string 直接就可以包含任意二进制数据
            lua.create_string(&buf)
        });

        methods.add_async_method_mut("write", |_, mut this, data: mlua::BString| async move {
            let n = this.conn.write(&data).await?;
            Ok(n)
        });

        methods.add_async_method_mut("close", |_, mut this, ()| async move {
            this.conn.shutdown().await?;
            Ok(())
        });

        methods.add_async_method_mut("flush", |_, mut this, ()| async move {
            this.conn.flush().await?;
            // debug!("flush ok");
            Ok(())
        });

        methods.add_method_mut("poll_flush", |_, this, cx_ll: LuaLightUserData| {
            let void = cx_ll.0;
            assert!(!void.is_null());

            let cx = unsafe { &mut *(void as *mut Context<'_>) };

            let x = this.conn.as_mut().poll_flush(cx);
            Ok(EmptyPollResult(x))
        });

        methods.add_method_mut("poll_close", |_, this, cx_ll: LuaLightUserData| {
            let void = cx_ll.0;
            assert!(!void.is_null());

            let cx = unsafe { &mut *(void as *mut Context<'_>) };

            let x = this.conn.as_mut().poll_shutdown(cx);
            Ok(EmptyPollResult(x))
        });

        methods.add_method_mut(
            "poll_write",
            |_, this, params: (LuaLightUserData, mlua::BString)| {
                let cx_void = params.0 .0;
                assert!(!cx_void.is_null());

                let cx = unsafe { &mut *(cx_void as *mut Context<'_>) };

                let x = this.conn.as_mut().poll_write(cx, &params.1);
                Ok(WritePollResult(x))
            },
        );

        methods.add_method_mut(
            "poll_read",
            |_, this, params: (LuaLightUserData, LuaLightUserData)| {
                let cx_void = params.0 .0;
                let buf_void = params.1 .0;
                assert!(!cx_void.is_null());
                assert!(!buf_void.is_null());

                let cx = unsafe { &mut *(cx_void as *mut Context<'_>) };

                let rb = unsafe { &mut *(buf_void as *mut ReadBuf<'_>) };

                // debug!("reading...");
                let r = this.conn.as_mut().poll_read(cx, rb);
                // debug!("read returned");

                Ok(EmptyPollResult(r))
            },
        );
    }
}

pub struct LuaConn {
    #[allow(unused)]
    lua: Lua, //就算不使用 lua, 也要带着，如果不带着，就会自动被释放掉, 导致LuaFunction不可用
    read_f: LuaFunction,
    write_f: LuaFunction,
    close_f: LuaFunction,
    flush_f: LuaFunction,
}

impl AsyncRead for LuaConn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let read_f = &self.read_f;

        let raw_ptr1 = cx as *mut Context<'_> as *mut c_void;

        let raw_ptr2 = buf as *mut ReadBuf<'_> as *mut c_void;

        let x = read_f
            .call::<i64>((LuaLightUserData(raw_ptr1), LuaLightUserData(raw_ptr2)))
            .unwrap();

        match x {
            -1 => Poll::Pending,
            -2 => Poll::Ready(Err(io::Error::other("some err"))),
            _ => Poll::Ready(Ok(())),
        }
    }
}

impl AsyncWrite for LuaConn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let wf = &self.write_f;

        let raw_ptr = cx as *mut Context<'_> as *mut c_void;

        let x = wf
            .call::<i64>((LuaLightUserData(raw_ptr), BString::from(buf)))
            .unwrap();

        match x {
            -1 => Poll::Pending,
            -2 => Poll::Ready(Err(io::Error::other("some err"))),
            n => Poll::Ready(Ok(n as usize)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        let ff = &self.flush_f;

        let raw_ptr = cx as *mut Context<'_> as *mut c_void;

        let f = ff.call_async(LuaLightUserData(raw_ptr));

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
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        // debug!("shutdown called");
        let close_f = &self.close_f;

        let raw_ptr = cx as *mut Context<'_> as *mut c_void;

        let f = close_f.call_async(LuaLightUserData(raw_ptr));

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

        let f = lua
            .create_function(|_, n: usize| Ok(ReadBufWrapper::new(n)))
            .unwrap();
        lua.globals().set("Create_read_buf", f).unwrap();

        let f = lua
            .create_function(|_, rb: LuaLightUserData| {
                let rbw = ReadBufWrapper {
                    real_buf: None,
                    ptr: rb,
                };

                Ok(rbw)
            })
            .unwrap();
        lua.globals().set("Wrap_read_buf", f).unwrap();

        match params.c {
            Stream::Conn(c) => {
                let a = match params.a {
                    Some(a) => AddrWrapper(a, false),
                    None => AddrWrapper(Addr::default(), true),
                };

                let conn_v = RustConn { conn: Box::pin(c) };

                let b = match params.b {
                    Some(b) => BytesMutWrapper(b, false),
                    None => BytesMutWrapper(BytesMut::new(), true),
                };

                let cid_v = cid.to_string();

                let bi: usize = behavior.into(); // 将 behavior enum 传成数字

                // 返回  {read, write, close, flush} ,然后后面代码再将其包装为 一个 LuaConn
                // 如果 不为 Table 而为一个 UserData, 则 其为 传入的 RustConn
                let r = handshake_f.call::<(Value, Value, Value)>((cid_v, bi, a, b, conn_v));
                match r {
                    Ok(r) => {
                        let c: Box<dyn ruci::net::AsyncConn> = match r.0 {
                            LuaValue::Table(table) => {
                                let read_f: LuaFunction = table.get(1).unwrap();
                                let write_f: LuaFunction = table.get(2).unwrap();
                                let close_f: LuaFunction = table.get(3).unwrap();
                                let flush_f: LuaFunction = table.get(4).unwrap();

                                let nc = LuaConn {
                                    lua,
                                    read_f,
                                    write_f,
                                    close_f,
                                    flush_f,
                                };
                                Box::new(nc)
                            }

                            LuaValue::UserData(any_user_data) => {
                                let b: RustConn = any_user_data.take().unwrap();
                                Box::new(b.conn)
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
