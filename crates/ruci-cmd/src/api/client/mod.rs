use super::*;

#[derive(Subcommand)]
pub enum Commands {
    Get1,
    Get2,

    /// stop server
    Stop,
}
pub async fn deal_cmds(_command: Option<Commands>) {}
