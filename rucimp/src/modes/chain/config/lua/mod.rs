/*!
Defines functions to load static chain from a lua file.
 */

#[cfg(test)]
#[allow(unused)]
pub mod test;

pub mod finite;
pub mod infinite;

use super::*;
use mlua::prelude::*;
use mlua::{Lua, LuaSerdeExt, Value};

const CONFIG_KEY: &str = "Config";

/// load chain::config::StaticConfig from a lua file which has a
/// "Config" global variable
///
/// if file_source is given, a Load_file function is registered for lua
/// so that the lua code can access to the related files.
pub fn load_static(
    lua_text: &str,
    file_source: Option<&crate::utils::FileSource>,
) -> mlua::Result<StaticConfig> {
    let lua = Lua::new();

    file_source.inspect(|file_source| crate::map::lua::create_load_file_func(&lua, file_source));

    lua.load(lua_text).exec()?;

    let ct: LuaTable = lua.globals().get(CONFIG_KEY)?;

    let c: StaticConfig = lua.from_value(Value::Table(ct))?;

    Ok(c)
}

/// return a `Lua` with `Config` set to c.
pub fn save_static(c: &StaticConfig) -> mlua::Result<Lua> {
    let lua = Lua::new();
    lua.globals().set(CONFIG_KEY, lua.to_value(c)?)?;
    Ok(lua)
}
