use ruci_cmd::run_main;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    run_main().await
}
