/*!
Defines functions to load static chain from a lua file.
 */

#[cfg(test)]
#[allow(unused)]
#[cfg(any(feature = "lua", feature = "lua54"))]
pub mod test;

pub mod finite;
pub mod infinite;

use super::*;
use mlua::prelude::*;
use mlua::{Lua, LuaSerdeExt, Value};

#[derive(Clone)]
pub struct LuaMapWrapper(Arc<MapBox>);

use mlua::UserData;

impl<'lua> FromLua<'lua> for LuaMapWrapper {
    fn from_lua(value: Value<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.take::<Self>()?),
            _ => unreachable!(),
        }
    }
}
use mlua::UserDataMethods;
impl UserData for LuaMapWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("clone", |_, m, ()| Ok(m.clone()));
    }
}

/// load chain::config::StaticConfig from a lua file which has a
/// "Config" global variable
pub fn load_static(lua_text: &str) -> mlua::Result<StaticConfig> {
    let lua = Lua::new();

    lua.load(lua_text).exec()?;

    let ct: LuaTable = lua.globals().get("Config")?;

    let c: StaticConfig = lua.from_value(Value::Table(ct))?;

    Ok(c)
}

/// return a `Lua` with `config` set to c.
pub fn save_static(c: &StaticConfig) -> mlua::Result<Lua> {
    let lua = Lua::new();
    lua.globals().set("config", lua.to_value(c)?)?;
    Ok(lua)
}
