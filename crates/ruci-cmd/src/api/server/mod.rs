use super::*;

#[derive(Subcommand)]
pub enum Commands {
    /// start api server
    Run,
}

pub async fn deal_cmds(_command: Option<Commands>) {}
