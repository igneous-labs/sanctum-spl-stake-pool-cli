use clap::Subcommand;

mod create_pool;
mod deposit_stake;
mod list;
mod sync_pool;
mod sync_validator_list;
mod update;
mod withdraw_stake;

pub use create_pool::*;
pub use deposit_stake::*;
pub use list::*;
pub use sync_pool::*;
pub use sync_validator_list::*;
pub use update::*;
pub use withdraw_stake::*;

#[derive(Debug, Subcommand)]
pub enum Subcmd {
    CreatePool(CreatePoolArgs),
    DepositStake(DepositStakeArgs),
    List(ListArgs),
    SyncPool(SyncPoolArgs),
    SyncValidatorList(SyncValidatorListArgs),
    Update(UpdateArgs),
    WithdrawStake(WithdrawStakeArgs),
}

impl Subcmd {
    pub async fn run(args: crate::Args) {
        match args.subcmd {
            Self::CreatePool(_) => CreatePoolArgs::run(args).await,
            Self::DepositStake(_) => DepositStakeArgs::run(args).await,
            Self::List(_) => ListArgs::run(args).await,
            Self::SyncPool(_) => SyncPoolArgs::run(args).await,
            Self::SyncValidatorList(_) => SyncValidatorListArgs::run(args).await,
            Self::Update(_) => UpdateArgs::run(args).await,
            Self::WithdrawStake(_) => WithdrawStakeArgs::run(args).await,
        }
    }
}
