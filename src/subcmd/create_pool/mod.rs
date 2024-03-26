use std::path::PathBuf;

use clap::Args;

#[derive(Args, Debug)]
#[command(long_about = "Create a new stake pool")]
pub struct CreatePoolArgs {
    #[arg(
        help = "Path to create-pool config file",
    )]
    pub config: PathBuf,
}

impl CreatePoolArgs {
    pub async fn run(_args: crate::Args) {
        todo!()
    }
}
