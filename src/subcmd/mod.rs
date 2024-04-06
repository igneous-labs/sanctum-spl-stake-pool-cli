use clap::Subcommand;

mod create_pool;
mod deposit_sol;
mod list;
mod sync_pool;
mod sync_validator_list;
mod update;

pub use create_pool::*;
pub use deposit_sol::*;
pub use list::*;
pub use sync_pool::*;
pub use sync_validator_list::*;
pub use update::*;

#[derive(Debug, Subcommand)]
pub enum Subcmd {
    CreatePool(CreatePoolArgs),
    DepositSol(DepositSolArgs),
    List(ListArgs),
    SyncPool(SyncPoolArgs),
    SyncValidatorList(SyncValidatorListArgs),
    Update(UpdateArgs),
}

impl Subcmd {
    pub async fn run(args: crate::Args) {
        match args.subcmd {
            Subcmd::CreatePool(_) => CreatePoolArgs::run(args).await,
            Subcmd::DepositSol(_) => DepositSolArgs::run(args).await,
            Subcmd::List(_) => ListArgs::run(args).await,
            Subcmd::SyncPool(_) => SyncPoolArgs::run(args).await,
            Subcmd::SyncValidatorList(_) => SyncValidatorListArgs::run(args).await,
            Subcmd::Update(_) => UpdateArgs::run(args).await,
        }
    }
}
