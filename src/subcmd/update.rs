use std::str::FromStr;

use borsh::BorshDeserialize;
use clap::{
    builder::{StringValueParser, TypedValueParser},
    Args,
};
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{clock::Clock, pubkey::Pubkey, sysvar};
use spl_stake_pool_interface::{StakePool, ValidatorList};

use crate::{
    update::{update_pool, UpdatePoolArgs},
    UpdateCtrl,
};

use super::Subcmd;

#[derive(Args, Debug)]
#[command(long_about = "Run the complete epoch update crank for a stake pool")]
pub struct UpdateArgs {
    #[arg(
        long,
        short,
        help = "How to run the update",
        default_value_t = UpdateCtrl::IfNeeded,
        value_enum,
    )]
    pub ctrl: UpdateCtrl,

    #[arg(
        long,
        short,
        help = "UpdateStakePoolBalance instruction's `no_merge` parameter",
        default_value_t = false
    )]
    pub no_merge: bool,

    #[arg(
        help = "Pubkey of the pool to update",
        value_parser = StringValueParser::new().try_map(|s| Pubkey::from_str(&s)),
    )]
    pub pool: Pubkey,
}

impl UpdateArgs {
    pub async fn run(args: crate::Args) {
        let Self {
            pool,
            ctrl,
            no_merge,
        } = match args.subcmd {
            Subcmd::Update(a) => a,
            _ => unreachable!(),
        };

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let mut fetched = rpc
            .get_multiple_accounts(&[pool, sysvar::clock::ID])
            .await
            .unwrap();
        let clock = fetched.pop().unwrap().unwrap();
        let stake_pool_acc = fetched.pop().unwrap().unwrap();

        let program_id = stake_pool_acc.owner;
        let Clock { epoch, .. } = bincode::deserialize(&clock.data).unwrap();
        let stake_pool = StakePool::deserialize(&mut stake_pool_acc.data.as_slice()).unwrap();

        let validator_list_acc = rpc.get_account(&stake_pool.validator_list).await.unwrap();

        let ValidatorList { validators, .. } =
            ValidatorList::deserialize(&mut validator_list_acc.data.as_slice()).unwrap();

        update_pool(UpdatePoolArgs {
            rpc: &rpc,
            send_mode: args.send_mode,
            payer: payer.as_ref(),
            program_id,
            current_epoch: epoch,
            stake_pool: Keyed {
                pubkey: pool,
                account: &stake_pool_acc,
            },
            validator_list_entries: &validators,
            fee_limit_cb: args.fee_limit_cb,
            ctrl,
            no_merge,
        })
        .await;
    }
}
