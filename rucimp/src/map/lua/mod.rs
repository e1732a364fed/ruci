/*!
Defines a Map that uses lua code as its maps method.

In order to let lua take full use of rust code, we have to wrap everything for lua.
 */

use std::sync::Arc;

use async_trait::async_trait;
use bytes::BufMut;
use bytes::BytesMut;
use macro_map::{map_ext_fields, MapExt};
use mlua::prelude::*;
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
// use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub struct LuaMapWrapper(pub Arc<MapBox>);

impl FromLua for LuaMapWrapper {
    fn from_lua(value: Value, _lua: &Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.take::<Self>()?),
            _ => unreachable!(),
        }
    }
}
impl UserData for LuaMapWrapper {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("clone", |_, m, ()| Ok(m.clone()));
    }
}

pub struct LuaAddr(pub Addr);

impl UserData for LuaAddr {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("network", |_, a, ()| Ok(a.0.network.to_string()));
    }
}

pub struct LuaBytesMut(pub BytesMut);

impl UserData for LuaBytesMut {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("put_u16", |_, b, u| Ok(b.0.put_u16(u)));
    }
}

pub struct LuaConn(pub ruci::net::Conn);

// impl UserData for LuaConn {
//     fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
//         // methods.add_method_mut(
//         //     "write_all",
//         //     |_, c, u| {
//         //         // let bb: mlua::BorrowedBytes = u;
//         //         Ok(())
//         //     }, // Ok(c.0.write_all(u))
//         // );
//     }
// }

#[map_ext_fields]
#[derive(Debug, Clone, MapExt, Default)]
pub struct LuaMap {
    lua_text: String,
    f_key: String,
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
        _behavior: map::ProxyBehavior,
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
        let handshake_f: LuaFunction = lua.globals().get(self.f_key.clone()).unwrap();

        match params.c {
            Stream::Conn(_c) => {
                if let Some(_a) = params.a {
                    //cid, c, a, params.b

                    let cid_v = lua.to_value(&cid).ok().unwrap();

                    let r = handshake_f.call::<(LuaString, Value)>((cid_v, 1));
                    match r {
                        Ok(_) => todo!(),
                        Err(e) => MapResult::from_e(e),
                    }
                } else {
                    MapResult::err_str("LuaMap client requires a target_addr, got None")
                }
            }
            _ => MapResult::err_str("LuaMap only support tcplike stream"),
        }
    }
}
