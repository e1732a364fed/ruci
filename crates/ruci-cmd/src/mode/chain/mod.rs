use rucimp::{
    example_common::{try_get_filecontent, wait_close_sig},
    modes::chain::config::lua,
};

///blocking
pub(crate) async fn run(f: &str) -> anyhow::Result<()> {
    let contents = try_get_filecontent(f)?;

    let mut se = rucimp::modes::chain::engine::Engine::default();
    let sc = lua::load(&contents).expect("has valid lua codes in the file content");

    se.init(sc);

    let se = Box::new(se);

    se.run().await?;

    wait_close_sig().await?;

    se.stop().await;
    Ok(())
}
