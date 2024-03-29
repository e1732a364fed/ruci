/*!
在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行
*/

use std::{env, time::Duration};

use rucimp::{modes::chain::engine::Engine, utils::*};
use tracing::debug;
mod shared;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version("example: chain");

    let default_fn = "local.lua".to_string();

    let args: Vec<String> = env::args().collect();

    let arg_f = if args.len() > 1 {
        Some(args[1].as_str())
    } else {
        None
    };

    let contents = try_get_file_content(&default_fn, arg_f)?;

    let mut e = Engine::default();

    e.init_lua(contents)?;

    let mut js = e.run().await?;

    wait_close_sig().await?;

    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        println!("Force shutdown after 3 secs!"); //only println works at this point.
        std::process::exit(1);
    });

    e.stop().await;

    debug!("Waiting for join set");

    let r = js.shutdown().await;

    debug!("{:?}", r);
    // js.shutdown().await;

    // js.abort_all();
    // while let Some(res) = js.join_next().await {
    //     debug!("{:?}", res)
    // }

    Ok(())
}
