use clap::Subcommand;

mod create_pool;
mod decrease_validator_stake;
mod deposit_sol;
mod deposit_stake;
mod increase_validator_stake;
mod list;
mod set_staker;
mod sync_delegation;
mod sync_pool;
mod sync_validator_list;
mod update;
mod withdraw_stake;

pub use create_pool::*;
pub use decrease_validator_stake::*;
pub use deposit_sol::*;
pub use deposit_stake::*;
pub use increase_validator_stake::*;
pub use list::*;
pub use set_staker::*;
pub use sync_delegation::*;
pub use sync_pool::*;
pub use sync_validator_list::*;
pub use update::*;
pub use withdraw_stake::*;

#[derive(Debug, Subcommand)]
pub enum Subcmd {
    CreatePool(CreatePoolArgs),
    DecreaseValidatorStake(DecreaseValidatorStakeArgs),
    DepositSol(DepositSolArgs),
    DepositStake(DepositStakeArgs),
    IncreaseValidatorStake(IncreaseValidatorStakeArgs),
    List(ListArgs),
    SetStaker(SetStakerArgs),
    SyncDelegation(SyncDelegationArgs),
    SyncPool(SyncPoolArgs),
    SyncValidatorList(SyncValidatorListArgs),
    Update(UpdateArgs),
    WithdrawStake(WithdrawStakeArgs),
}

impl Subcmd {
    pub async fn run(args: crate::Args) {
        match args.subcmd {
            Self::CreatePool(_) => CreatePoolArgs::run(args).await,
            Self::DecreaseValidatorStake(_) => DecreaseValidatorStakeArgs::run(args).await,
            Self::DepositSol(_) => DepositSolArgs::run(args).await,
            Self::DepositStake(_) => DepositStakeArgs::run(args).await,
            Self::IncreaseValidatorStake(_) => IncreaseValidatorStakeArgs::run(args).await,
            Self::List(_) => ListArgs::run(args).await,
            Self::SetStaker(_) => SetStakerArgs::run(args).await,
            Self::SyncDelegation(_) => SyncDelegationArgs::run(args).await,
            Self::SyncPool(_) => SyncPoolArgs::run(args).await,
            Self::SyncValidatorList(_) => SyncValidatorListArgs::run(args).await,
            Self::Update(_) => UpdateArgs::run(args).await,
            Self::WithdrawStake(_) => WithdrawStakeArgs::run(args).await,
        }
    }
}
