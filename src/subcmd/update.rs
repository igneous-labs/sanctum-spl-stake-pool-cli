use std::str::FromStr;

use borsh::BorshDeserialize;
use clap::{
    builder::{StringValueParser, TypedValueParser},
    Args,
};
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{epoch_info::EpochInfo, pubkey::Pubkey};
use spl_stake_pool_interface::{StakePool, ValidatorList};

use crate::update::{update_pool_if_needed, UpdatePoolIfNeededArgs};

use super::Subcmd;

#[derive(Args, Debug)]
#[command(long_about = "Run the complete epoch update crank for a stake pool")]
pub struct UpdateArgs {
    #[arg(
        help = "Pubkey of the pool to update",
        value_parser = StringValueParser::new().try_map(|s| Pubkey::from_str(&s)),
    )]
    pub pool: Pubkey,
}

impl UpdateArgs {
    pub async fn run(args: crate::Args) {
        let Self { pool } = match args.subcmd {
            Subcmd::Update(a) => a,
            _ => unreachable!(),
        };

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let stake_pool_acc = rpc.get_account(&pool).await.unwrap();
        let stake_pool = StakePool::deserialize(&mut stake_pool_acc.data.as_slice()).unwrap();
        let validator_list_acc = rpc.get_account(&stake_pool.validator_list).await.unwrap();
        let ValidatorList { validators, .. } =
            ValidatorList::deserialize(&mut validator_list_acc.data.as_slice()).unwrap();

        let EpochInfo { epoch, .. } = rpc.get_epoch_info().await.unwrap();

        update_pool_if_needed(UpdatePoolIfNeededArgs {
            rpc: &rpc,
            send_mode: args.send_mode,
            payer: payer.as_ref(),
            program_id: args.program,
            current_epoch: epoch,
            stake_pool: Keyed {
                pubkey: pool,
                account: &stake_pool_acc,
            },
            validator_list_entries: &validators,
            fee_limit_cu: args.fee_limit_cu,
        })
        .await;
    }
}
