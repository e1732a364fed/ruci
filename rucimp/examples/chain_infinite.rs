/*!
* 在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 infinite chain 模式运行。
*/

use std::env;

use rucimp::{modes::chain::engine::Engine, utils::*};
mod shared;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version("example: chain");

    let default_fn = "local.lua".to_string();

    let args: Vec<String> = env::args().collect();

    let arg_f = if args.len() > 1 && args[1] != "-s" {
        Some(args[1].as_str())
    } else {
        None
    };

    let contents = try_get_file_content(&default_fn, arg_f)?;

    let mut se = Engine::default();

    se.init_lua_infinite_dynamic(contents)?;

    let mut js = se.run().await?;

    wait_close_sig().await?;

    se.stop().await;

    js.shutdown().await;

    Ok(())
}
