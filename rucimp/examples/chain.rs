/*!
* 在 working dir 或 working dir /resource 或 ../resource/ 文件夹查找 local.lua 或
 用户提供的参数作为配置文件 读取它并以 chain 模式运行。
*/

use rucimp::{
    chain::{config::lua, engine::Engine},
    example_common::*,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    print_env_version("chain");

    let default_fn = "local.lua".to_string();

    let contents = try_get_filecontent(&default_fn)?;

    let mut se = Engine::default();
    let sc = lua::load(&contents).expect("has valid lua codes in the file content");

    se.init(sc);

    let se = Box::new(se);

    se.run().await?;

    wait_close_sig().await?;

    se.stop().await;

    Ok(())
}
