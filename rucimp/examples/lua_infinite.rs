/*!
在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 infinite chain 模式运行.
*/

use std::env;

use rucimp::{modes::chain::engine::Engine, utils::*, DEFAULT_LUA_CONFIG_FILE_NAME};
mod shared;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version_and_init_log("example: chain_infinite");

    let default_fn = DEFAULT_LUA_CONFIG_FILE_NAME.to_string();

    let args: Vec<String> = env::args().collect();

    let arg_f = if args.len() > 1 {
        Some(args[1].as_str())
    } else {
        None
    };

    let bs = try_get_file_content(&default_fn, arg_f)?;
    let contents = String::from_utf8_lossy(bs.as_slice()).to_string();

    Engine::new_and_run(Box::new(move |e| {
        e.set_default_file_source();
        e.init_lua_infinite_dynamic(contents)
    }))
    .await
}
