use std::{num::NonZeroU32, path::PathBuf, str::FromStr};

use borsh::BorshDeserialize;
use clap::Args;
use sanctum_solana_cli_utils::{parse_signer, PubkeySrc, TxSendMode};
use sanctum_spl_stake_pool_lib::{FindValidatorStakeAccount, FindValidatorStakeAccountArgs};
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
    long_about = "(Staker only) sync validator list entries and preferred validators with a pool config file"
)]
pub struct SyncValidatorListArgs {
    #[arg(
        help = "Path to pool config file containing the updated validator list and preferred validators to update the pool to"
    )]
    pub pool_config: PathBuf,
}

impl SyncValidatorListArgs {
    pub async fn run(args: crate::Args) {
        let Self { pool_config } = match args.subcmd {
            Subcmd::SyncValidatorList(a) => a,
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

        // need to update first to be able to add/remove validators
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
            validator_list_entries: &old_validators,
            fee_limit_cb: args.fee_limit_cb,
            ctrl: UpdateCtrl::IfNeeded,
        })
        .await;

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

        let (add, remove) = svlc.add_remove_changeset(&old_validators);
        // need to additionally fetch VSAs of validators to remove to make sure they weren't
        // already DeactivateDelinquent'd
        let remove_vsas: Vec<Pubkey> = remove
            .clone()
            .map(|vsi| {
                FindValidatorStakeAccount::new(FindValidatorStakeAccountArgs {
                    pool,
                    vote: vsi.vote_account_address,
                    seed: NonZeroU32::new(vsi.validator_seed_suffix),
                })
                .run_for_prog(&program_id)
                .0
            })
            .collect();
        let remove_vsas = rpc
            .get_multiple_accounts(&remove_vsas)
            .await
            .unwrap()
            .into_iter()
            .map(|acc_opt| bincode::deserialize(&acc_opt.unwrap().data).unwrap());

        print_removing_validators_msg(remove.clone());

        for remove_validator_ix_chunk in svlc
            .remove_validators_ixs(remove.zip(remove_vsas))
            .unwrap()
            .as_slice()
            .chunks(MAX_REMOVE_VALIDATOR_IXS_ENUM_PER_TX)
        {
            let remove_validator_ix_chunk = match args.send_mode {
                TxSendMode::DumpMsg => Vec::from(remove_validator_ix_chunk),
                _ => {
                    with_auto_cb_ixs(
                        &rpc,
                        &payer.pubkey(),
                        Vec::from(remove_validator_ix_chunk),
                        &[],
                        args.fee_limit_cb,
                    )
                    .await
                }
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &remove_validator_ix_chunk,
                &[],
                &mut svlc.signers_maybe_dup(),
            )
            .await;
        }

        print_adding_validators_msg(add.clone());

        for add_validator_ix_chunk in svlc
            .add_validators_ixs(add)
            .unwrap()
            .as_slice()
            .chunks(MAX_ADD_VALIDATORS_IX_PER_TX)
        {
            let add_validator_ix_chunk = match args.send_mode {
                TxSendMode::DumpMsg => Vec::from(add_validator_ix_chunk),
                _ => {
                    with_auto_cb_ixs(
                        &rpc,
                        &payer.pubkey(),
                        Vec::from(add_validator_ix_chunk),
                        &[],
                        args.fee_limit_cb,
                    )
                    .await
                }
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &add_validator_ix_chunk,
                &[],
                &mut svlc.signers_maybe_dup(),
            )
            .await;
        }

        let preferred_validator_changes = svlc.preferred_validator_changeset(&stake_pool);
        for change in preferred_validator_changes.clone() {
            eprintln!("{change}");
        }
        let preferred_validator_ixs = svlc
            .preferred_validator_ixs(preferred_validator_changes)
            .unwrap();
        if !preferred_validator_ixs.is_empty() {
            let preferred_validator_ixs = match args.send_mode {
                TxSendMode::DumpMsg => preferred_validator_ixs,
                _ => {
                    with_auto_cb_ixs(
                        &rpc,
                        &payer.pubkey(),
                        preferred_validator_ixs,
                        &[],
                        args.fee_limit_cb,
                    )
                    .await
                }
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &preferred_validator_ixs,
                &[],
                &mut svlc.signers_maybe_dup(),
            )
            .await;
        }
    }
}
