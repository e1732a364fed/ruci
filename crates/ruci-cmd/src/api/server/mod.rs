mod folder_serve;

use super::*;

#[derive(Subcommand)]
pub enum Commands {
    /// start api server
    Run,

    /// serve files in folder "static"
    FileServer,
}

pub async fn deal_cmds(command: Option<Commands>) {
    let cmd = match command {
        Some(c) => c,
        None => return,
    };
    match cmd {
        Commands::Run => todo!(),
        Commands::FileServer => folder_serve::serve_static().await,
    }
}
