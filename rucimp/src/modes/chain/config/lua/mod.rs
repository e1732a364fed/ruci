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

    file_source.inspect(|file_source| create_load_file_func(&lua, file_source));

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

pub fn create_load_file_func(lua: &Lua, file_source: &crate::utils::FileSource) {
    let raw_ptr = file_source as *const crate::utils::FileSource as *const std::os::raw::c_void;

    let pointer_n = raw_ptr as usize;

    let f = lua
        .create_function(move |lua, s: mlua::BString| {
            let file_name = std::str::from_utf8(s.as_slice()).unwrap();

            let file_source = unsafe {
                &*((pointer_n as *const std::os::raw::c_void) as *const crate::utils::FileSource)
            };

            let r = file_source
                .get_file_content(file_name)
                .map_err(|e| mlua::Error::external(e))?;

            lua.create_string(&r.0)
        })
        .unwrap();
    lua.globals().set("Load_file", f).unwrap();
}
