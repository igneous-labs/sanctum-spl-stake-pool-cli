use clap::Subcommand;

mod create_pool;
mod list;
mod sync_pool;
mod update;

pub use create_pool::*;
pub use list::*;
pub use sync_pool::*;
pub use update::*;

#[derive(Debug, Subcommand)]
pub enum Subcmd {
    CreatePool(CreatePoolArgs),
    List(ListArgs),
    SyncPool(SyncPoolArgs),
    Update(UpdateArgs),
}

impl Subcmd {
    pub async fn run(args: crate::Args) {
        match args.subcmd {
            Subcmd::CreatePool(_) => CreatePoolArgs::run(args).await,
            Subcmd::List(_) => ListArgs::run(args).await,
            Subcmd::SyncPool(_) => SyncPoolArgs::run(args).await,
            Subcmd::Update(_) => UpdateArgs::run(args).await,
        }
    }
}
