/*!
Defines functions to load static chain configuration from a lua file.
*/

#[cfg(test)]
#[allow(unused)]
pub mod test;

// pub mod finite;
pub mod infinite;

use super::*;
pub use mlua;
use mlua::prelude::*;
use mlua::{Lua, LuaSerdeExt, Value};

const CONFIG_KEY: &str = "Config";

/// Loads chain::config::StaticConfig from a lua file that contains a "Config" global variable.
/// If file_source is provided, a Load_file function will be registered to allow lua code to access related files.
///
/// # Arguments
/// * `lua_text` - The lua configuration file content
/// * `file_source` - Optional file source for loading additional files
///
/// # Returns
/// * `mlua::Result<StaticConfig>` - The parsed static chain configuration
pub fn load_static(
    lua_text: &str,
    file_source: Arc<crate::utils::FileSource>,
) -> anyhow::Result<StaticConfig> {
    let lua = Lua::new();
    use anyhow::Context;
    crate::map::lua::create_load_file_func(&lua, file_source.as_ref());

    lua.load(lua_text)
        .exec()
        .map_err(|e| anyhow::anyhow!("run and load lua failed: {}", e))?;

    let ct: LuaTable = lua
        .globals()
        .get(CONFIG_KEY)
        .map_err(|e| anyhow::anyhow!("get lua global Config failed: {}", e))
        .context("get lua global Config failed")?;

    let dr = mlua::serde::de::Deserializer::new(Value::Table(ct));

    let c: StaticConfig =
        serde_path_to_error::deserialize(dr).context("Config to StaticConfig failed")?;

    // let c: StaticConfig = lua
    //     .from_value(Value::Table(ct))
    //     .context("Config to StaticConfig failed")?;

    Ok(c)
}

/// Returns a new Lua instance with the given static configuration saved as a global "Config" variable
///
/// # Arguments
/// * `c` - The static configuration to save
///
/// # Returns
/// * `mlua::Result<Lua>` - A new Lua instance with the config saved
pub fn save_static(c: &StaticConfig) -> mlua::Result<Lua> {
    let lua = Lua::new();
    lua.globals().set(CONFIG_KEY, lua.to_value(c)?)?;
    Ok(lua)
}

/// Converts a Lua value to a formatted Lua code string
///
/// # Arguments
/// * `value` - The Lua value to convert
///
/// # Returns
/// * `anyhow::Result<String>` - The formatted Lua code string
pub fn lua_value_to_string(value: &Value) -> anyhow::Result<String> {
    let s = serde_lua_table::to_string_pretty(&value)?;
    Ok(s)
}

/// Converts a Lua value to a formatted Lua code string with a prefix
///
/// # Arguments
/// * `value` - The Lua value to convert
/// * `prefix` - The prefix to add at the beginning
///
/// # Returns
/// * `anyhow::Result<String>` - The formatted Lua code string with prefix
pub fn lua_value_to_string_with_prefix(value: &Value, prefix: &str) -> anyhow::Result<String> {
    let mut s = lua_value_to_string(value)?;
    s.insert_str(0, prefix);
    Ok(s)
}
