use std::time::Duration;

use anyhow::Context;

use super::*;
use anyhow::Result;

#[derive(Subcommand, Clone)]
pub enum Commands {
    ConnectionCount {
        addr: Option<String>,
    },

    /// stop server
    Stop {
        addr: Option<String>,
    },
}
pub async fn deal_cmds(command: Option<Commands>) -> anyhow::Result<()> {
    let cmd = match command {
        Some(c) => c,
        None => return Ok(()),
    };
    fn get_real_addr(addr: Option<String>) -> String {
        addr.unwrap_or_else(|| String::from("http://") + DEFAULT_API_ADDR)
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

            let response = timeout_get(ad, "/cc").await?;

            println!("cc:{}", response.text().await?)
        }
        Commands::Stop { addr } => {
            let ad = get_real_addr(addr);

            let response = timeout_get(ad, "/stop_core").await?;

            println!("response:{}", response.text().await?)
        }
    };

    Ok(())
}
