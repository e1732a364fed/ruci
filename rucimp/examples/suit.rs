/*!
 * 在working dir 或 working dir /resource 文件夹查找 local.suit.toml 文件, 读取它并以suit模式运行。
 */

use std::env;

use rucimp::{
    modes::suit::{config::adapter::*, engine::SuitEngine},
    utils::*,
};
use tracing::warn;

mod shared;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::print_env_version("suit");

    let default_fn = "local.suit.toml".to_string();

    let args: Vec<String> = env::args().collect();

    let arg_f = if args.len() > 1 {
        Some(args[1].as_str())
    } else {
        None
    };

    let contents = try_get_file_content(&default_fn, arg_f)?;

    println!("{}", contents);
    let mut se = SuitEngine::default();

    se.load_config_from_str(
        &contents,
        load_in_mappers_by_str_and_ld_config,
        load_out_mappers_by_str_and_ld_config,
    );
    let r = se.block_run().await;

    warn!("suit engine run returns: {:?}", r);

    Ok(())
}
