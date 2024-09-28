use std::{path::PathBuf, str::FromStr};

use borsh::BorshDeserialize;
use clap::Args;
use sanctum_solana_cli_utils::{parse_signer, PubkeySrc, TxSendMode};
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{clock::Clock, pubkey::Pubkey, rent::Rent, sysvar};
use spl_stake_pool_interface::{StakePool, ValidatorList};

use crate::{
    pool_config::{
        print_adding_validators_msg, print_removing_validators_msg, ConfigRaw,
        SyncValidatorListConfig,
    },
    tx_utils::{
        handle_tx_full, with_auto_cb_ixs, MAX_ADD_VALIDATORS_IX_PER_TX,
        MAX_REMOVE_VALIDATOR_IXS_ENUM_PER_TX,
    },
    update::{update_pool, UpdatePoolArgs},
    UpdateCtrl,
};

use super::Subcmd;

#[derive(Args, Debug)]
#[command(
    long_about = "Decrease validator Stake from Stake Pool by lamports"
)]
pub struct DecreaseValidatorStakeArgs {
    #[arg(
        help = "Path to pool config file containing the current validator list and preferred validators for the pool"
    )]
    pub pool_config: PathBuf,
    #[arg(
        help = "The validator vote account to decrease stake from"
    )]
    pub validator: String,
    #[arg(
        help = "The amount in lamports to decrease the stake by"
    )]
    pub lamports_to_decrease: u64,
}

impl DecreaseValidatorStakeArgs {
    pub async fn run(args: crate::Args) {
        let Self { pool_config, validator, lamports_to_decrease } = match args.subcmd {
            Subcmd::DecreaseValidatorStake(a) => a,
            _ => unreachable!(),
        };

        let ConfigRaw {
            preferred_deposit_validator,
            preferred_withdraw_validator,
            validators,
            pool,
            staker,
            ..
        } = ConfigRaw::read_from_path(pool_config).unwrap();

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let staker = staker.map_or_else(|| None, |s| parse_signer(&s).ok()); // if staker is not a valid signer, treat it as None and fall back to payer
        let staker = staker
            .as_ref()
            .map_or_else(|| payer.as_ref(), |s| s.as_ref());
        let [preferred_deposit_validator, preferred_withdraw_validator] =
            [preferred_deposit_validator, preferred_withdraw_validator]
                .map(|opt| opt.map(|s| PubkeySrc::parse(&s).unwrap().pubkey()));

        let pool = PubkeySrc::parse(pool.as_ref().unwrap()).unwrap().pubkey();

        let mut fetched = rpc
            .get_multiple_accounts(&[pool, sysvar::clock::ID, sysvar::rent::ID])
            .await
            .unwrap();
        let rent = fetched.pop().unwrap().unwrap();
        let clock = fetched.pop().unwrap().unwrap();
        let stake_pool_acc = fetched.pop().unwrap().unwrap();
        let program_id = stake_pool_acc.owner;

        let rent: Rent = bincode::deserialize(&rent.data).unwrap();
        let Clock { epoch, .. } = bincode::deserialize(&clock.data).unwrap();
        let stake_pool = StakePool::deserialize(&mut stake_pool_acc.data.as_slice()).unwrap();

        let validator_list_acc = rpc.get_account(&stake_pool.validator_list).await.unwrap();
        let ValidatorList {
            validators: old_validators,
            ..
        } = ValidatorList::deserialize(&mut validator_list_acc.data.as_slice()).unwrap();

        let svlc = SyncValidatorListConfig {
            program_id,
            payer: payer.as_ref(),
            staker,
            pool,
            validator_list: stake_pool.validator_list,
            reserve: stake_pool.reserve_stake,
            preferred_deposit_validator,
            preferred_withdraw_validator,
            validators: validators
                .unwrap_or_default()
                .into_iter()
                .map(|v| Pubkey::from_str(&v.vote).unwrap())
                .collect(),
            rent: &rent,
        };

        let to_decrease = &old_validators.iter().filter(|vsi| &vsi.vote_account_address == &Pubkey::from_str(&validator).unwrap());

        for decrease_validator_stake_chunk_ix in svlc
            .decrease_validators_stake_ixs(to_decrease.clone(), lamports_to_decrease)
            .unwrap()
            .as_slice()
            .chunks(MAX_REMOVE_VALIDATOR_IXS_ENUM_PER_TX)
        {
            let decrease_validator_stake_chunk_ix = match args.send_mode {
                TxSendMode::DumpMsg => Vec::from(decrease_validator_stake_chunk_ix),
                _ => {
                    with_auto_cb_ixs(
                        &rpc,
                        &payer.pubkey(),
                        Vec::from(decrease_validator_stake_chunk_ix),
                        &[],
                        args.fee_limit_cb,
                    )
                    .await
                }
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &decrease_validator_stake_chunk_ix,
                &[],
                &mut svlc.signers_maybe_dup(),
            )
            .await;
        }
    }
}
