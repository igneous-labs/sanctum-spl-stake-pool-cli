use std::{cmp::Ordering, path::PathBuf};

use borsh::BorshDeserialize;
use clap::Args;
use itertools::Itertools;
use sanctum_solana_cli_utils::{PubkeySrc, TxSendMode};
use sanctum_spl_stake_pool_lib::{
    FindTransientStakeAccount, FindTransientStakeAccountArgs, FindValidatorStakeAccount,
};
use solana_sdk::{clock::Clock, pubkey::Pubkey, rent::Rent, stake::state::StakeStateV2, sysvar};
use spl_stake_pool_interface::{StakePool, ValidatorList, ValidatorStakeInfo};

use crate::{
    handle_tx_full, is_delegation_scheme_valid, parse_signer_pubkey_none, with_auto_cb_ixs,
    SyncDelegationConfig, SyncDelegationConfigToml, ValidatorDelegation, ValidatorDelegationTarget,
    MAX_INCREASE_VALIDATOR_STAKE_IX_PER_TX,
};

use super::Subcmd;

#[derive(Args, Debug)]
#[command(long_about = "(Staker only) sync target stake delegation amounts")]
pub struct SyncDelegationArgs {
    #[arg(help = "Path to sync delegation config file")]
    pub sync_delegation_config: PathBuf,
}

impl SyncDelegationArgs {
    pub async fn run(args: crate::Args) {
        let Self {
            sync_delegation_config,
        } = match args.subcmd {
            Subcmd::SyncDelegation(a) => a,
            _ => unreachable!(),
        };

        let SyncDelegationConfigToml {
            pool,
            staker,
            validators: delegation_scheme,
        } = SyncDelegationConfigToml::read_from_path(sync_delegation_config).unwrap();
        let mut delegation_scheme: Vec<ValidatorDelegation> = delegation_scheme
            .into_iter()
            .map(|s| s.try_into().unwrap())
            .collect();
        // move Remainder to the end
        // TODO: kinda jank to rely on this sorting behaviour to ensure
        // correct handling of Remainder
        delegation_scheme.sort_by(|a, b| {
            if matches!(a.target, ValidatorDelegationTarget::Remainder) {
                Ordering::Greater
            } else if matches!(b.target, ValidatorDelegationTarget::Remainder) {
                Ordering::Less
            } else {
                // preserve og order
                Ordering::Equal
            }
        });
        is_delegation_scheme_valid(delegation_scheme.iter().map(|v| &v.target)).unwrap();

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let pool = PubkeySrc::parse(&pool).unwrap().pubkey();

        let staker = staker
            .as_ref()
            .map_or_else(|| None, |s| parse_signer_pubkey_none(s).unwrap());
        let staker = staker
            .as_ref()
            .map_or_else(|| payer.as_ref(), |s| s.as_ref());

        let mut fetched = rpc
            .get_multiple_accounts(&[pool, sysvar::clock::ID, sysvar::rent::ID])
            .await
            .unwrap();
        let rent = fetched.pop().unwrap().unwrap();
        let clock = fetched.pop().unwrap().unwrap();
        let stake_pool_acc = fetched.pop().unwrap().unwrap();
        let program_id = stake_pool_acc.owner;

        let rent: Rent = bincode::deserialize(&rent.data).unwrap();
        let Clock {
            epoch: curr_epoch, ..
        } = bincode::deserialize(&clock.data).unwrap();
        let stake_pool = StakePool::deserialize(&mut stake_pool_acc.data.as_slice()).unwrap();

        if staker.pubkey() != stake_pool.staker {
            panic!(
                "Wrong staker. Expecting {}, got {}",
                stake_pool.staker,
                staker.pubkey()
            );
        }

        let mut fetched = rpc
            .get_multiple_accounts(&[stake_pool.validator_list, stake_pool.reserve_stake])
            .await
            .unwrap();
        let reserve_acc = fetched.pop().unwrap().unwrap();
        let validator_list_acc = fetched.pop().unwrap().unwrap();

        let ValidatorList { validators, .. } =
            ValidatorList::deserialize(&mut validator_list_acc.data.as_slice()).unwrap();

        let vsis: Vec<&ValidatorStakeInfo> = delegation_scheme
            .iter()
            .map(|ValidatorDelegation { vote, .. }| {
                validators
                    .iter()
                    .find(|vsi| vsi.vote_account_address == *vote)
                    .unwrap_or_else(|| panic!("Validator {vote} not part of pool"))
            })
            .collect();
        let stake_accs: Vec<Pubkey> = vsis
            .iter()
            .flat_map(
                |ValidatorStakeInfo {
                     vote_account_address,
                     transient_seed_suffix,
                     ..
                 }| {
                    let (vsa_pubkey, _bump) = FindValidatorStakeAccount {
                        pool,
                        vote: *vote_account_address,
                        seed: None,
                    }
                    .run_for_prog(&program_id);
                    let (tsa_pubkey, _bump) =
                        FindTransientStakeAccount::new(FindTransientStakeAccountArgs {
                            pool,
                            vote: *vote_account_address,
                            seed: *transient_seed_suffix,
                        })
                        .run_for_prog(&program_id);
                    [vsa_pubkey, tsa_pubkey]
                },
            )
            .collect();
        let fetched = rpc.get_multiple_accounts(&stake_accs).await.unwrap();
        let fetched_stake_accs: Vec<(StakeStateV2, Option<StakeStateV2>)> = fetched
            .chunks(2)
            .map(|a| {
                (
                    StakeStateV2::deserialize(&mut a[0].as_ref().unwrap().data.as_slice()).unwrap(),
                    a[1].as_ref()
                        .map(|a| StakeStateV2::deserialize(&mut a.data.as_slice()).unwrap()),
                )
            })
            .collect();

        let change_srcs = delegation_scheme
            .iter()
            .zip(vsis)
            .zip(fetched_stake_accs.iter())
            .map(|((scheme, vsi), (vsa, tsa))| {
                let target_stake = match scheme.target {
                    ValidatorDelegationTarget::Lamports(lamports) => lamports,
                    // TODO: u64::MAX ensures + having the remainder entry at the end of the array
                    // ensures correct behaviour but the terminal will always print a shortfall msg
                    ValidatorDelegationTarget::Remainder => u64::MAX,
                };
                (vsi, vsa, tsa, target_stake)
            });

        let sdc = SyncDelegationConfig {
            program_id,
            payer: payer.as_ref(),
            staker,
            pool,
            validator_list: stake_pool.validator_list,
            reserve: stake_pool.reserve_stake,
            reserve_lamports: reserve_acc.lamports,
            curr_epoch,
            rent,
        };

        let changes = sdc.changeset(change_srcs);
        changes.print_all_changes();

        // IncreaseAdditionalValidatorStake is worst case, takes 14 account inputs vs Decrease's 11
        for ix_chunk in &sdc
            .sync_delegation_ixs(changes)
            .chunks(MAX_INCREASE_VALIDATOR_STAKE_IX_PER_TX)
        {
            let ix_chunk = match args.send_mode {
                TxSendMode::DumpMsg => ix_chunk.collect(),
                _ => {
                    with_auto_cb_ixs(
                        &rpc,
                        &payer.pubkey(),
                        ix_chunk.collect(),
                        &[],
                        args.fee_limit_cb,
                    )
                    .await
                }
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &ix_chunk,
                &[],
                &mut sdc.signers_maybe_dup(),
            )
            .await;
        }
    }
}
