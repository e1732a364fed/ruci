/*!
 * 在working dir 或 working dir /resource 文件夹查找 local.suit.toml 文件, 读取它并以suit模式运行。
 */

use log::warn;
use rucimp::{
    example_common::*,
    suit::{config::adapter::*, engine::SuitEngine},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    //it's the second impl version of suit

    print_env_version("suit2");

    let default_fn = "local.suit.toml".to_string();

    let contents = try_get_filecontent(&default_fn)?;

    println!("{}", contents);
    let mut se = SuitEngine::new();

    se.load_config_from_str(
        &contents,
        load_in_mappers_by_str_and_ldconfig,
        load_out_mappers_by_str_and_ldconfig,
    );
    let r = se.block_run().await;

    warn!("suit engine run returns: {:?}", r);

    Ok(())
}
