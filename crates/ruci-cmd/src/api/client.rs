use std::time::Duration;

use anyhow::Context;

use anyhow::Result;
use clap::Subcommand;
use serde::Deserialize;
use serde::Serialize;

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum Commands {
    /// check all connection count
    ConnectionCount { addr: Option<String> },

    /// stop the running engine. This won't stop the api-server.
    Stop { addr: Option<String> },
}
pub async fn deal_cmds(command: Option<Commands>) -> anyhow::Result<()> {
    let cmd = match command {
        Some(c) => c,
        None => return Ok(()),
    };
    fn get_real_addr(addr: Option<String>) -> String {
        addr.unwrap_or_else(|| String::from("http://") + rucimp::DEFAULT_API_ADDR)
    }
    async fn timeout_get(ad: String, url: &str) -> Result<reqwest::Response> {
        Ok(
            tokio::time::timeout(Duration::from_secs(10), reqwest::get(ad + url))
                .await
                .context("request waiting for too long")??,
        )
    }

    match cmd {
        Commands::ConnectionCount { addr } => {
            let ad = get_real_addr(addr);

            let response = timeout_get(ad, "/api/connections/count").await?;

            println!("api/connections/count:{}", response.text().await?)
        }
        Commands::Stop { addr } => {
            let ad = get_real_addr(addr);

            let response = timeout_get(ad, "/api/engine/stop").await?;

            println!("response:{}", response.text().await?)
        }
    };

    Ok(())
}
