/*!
在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行
*/

use std::{env, time::Duration};

use rucimp::{modes::chain::engine::Engine, utils::*, DEFAULT_CONFIG_FILE_NAME};
use tracing::debug;
mod shared;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version_and_init_log("example: chain");

    let default_fn = DEFAULT_CONFIG_FILE_NAME.to_string();

    let args: Vec<String> = env::args().collect();

    let arg_f = if args.len() > 1 {
        Some(args[1].as_str())
    } else {
        None
    };

    let bs = try_get_file_content(&default_fn, arg_f)?;
    let contents = String::from_utf8_lossy(bs.as_slice()).to_string();

    let mut e = Engine::new();

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
