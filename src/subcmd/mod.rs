use clap::Subcommand;

mod create_pool;
mod list;

pub use create_pool::*;
pub use list::*;

#[derive(Debug, Subcommand)]
pub enum Subcmd {
    CreatePool(CreatePoolArgs),
    List(ListArgs),
}

impl Subcmd {
    pub async fn run(args: crate::Args) {
        match args.subcmd {
            Subcmd::CreatePool(_) => CreatePoolArgs::run(args).await,
            Subcmd::List(_) => ListArgs::run(args).await,
        }
    }
}
