#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ruci_cmd::run_main().await
}
