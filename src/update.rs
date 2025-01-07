use std::fmt::Display;

use borsh::BorshDeserialize;
use clap::ValueEnum;
use sanctum_solana_cli_utils::TxSendMode;
use sanctum_spl_stake_pool_lib::account_resolvers::{
    CleanupRemovedValidatorEntries, UpdateStakePoolBalance, UpdateValidatorListBalance,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{account::Account, pubkey::Pubkey, signer::Signer};
use spl_stake_pool_interface::{
    cleanup_removed_validator_entries_ix_with_program_id,
    update_stake_pool_balance_ix_with_program_id, StakePool, UpdateValidatorListBalanceIxArgs,
    ValidatorStakeInfo,
};

use crate::tx_utils::{handle_tx_full, with_auto_cb_ixs};

const MAX_VALIDATORS_TO_UPDATE_PER_TX: usize = 11;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum UpdateCtrl {
    /// Only run the update for parts of the pool that need it for this epoch
    #[default]
    IfNeeded,

    /// Force update the pool account only (not the validator list), even if it has been updated for this epoch
    ForcePool,

    /// Force update both the pool account and the entire validator list, even if the pool has been updated for this epoch
    ForceAll,
}

impl Display for UpdateCtrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IfNeeded => f.write_str("if-needed"),
            Self::ForcePool => f.write_str("force-pool"),
            Self::ForceAll => f.write_str("force-all"),
        }
    }
}

pub struct UpdatePoolArgs<'a> {
    pub rpc: &'a RpcClient,
    pub send_mode: TxSendMode,
    pub payer: &'a (dyn Signer + 'static),
    pub program_id: Pubkey,
    pub current_epoch: u64,
    pub stake_pool: Keyed<&'a Account>,
    pub validator_list_entries: &'a [ValidatorStakeInfo],
    pub fee_limit_cb: u64,
    pub ctrl: UpdateCtrl,
}

// ignores entries already updated for this epoch
pub async fn update_pool(
    UpdatePoolArgs {
        rpc,
        send_mode,
        payer,
        program_id,
        current_epoch,
        stake_pool,
        validator_list_entries,
        fee_limit_cb,
        ctrl,
    }: UpdatePoolArgs<'_>,
) {
    let sp = StakePool::deserialize(&mut stake_pool.account.data.as_slice()).unwrap();
    let is_updated_for_curr_epoch = sp.last_update_epoch >= current_epoch;
    if is_updated_for_curr_epoch && ctrl == UpdateCtrl::IfNeeded {
        eprintln!("Update not required");
        return;
    }
    eprintln!("Updating pool");

    // Update validator list:
    if !is_updated_for_curr_epoch || ctrl == UpdateCtrl::ForceAll {
        let uvlb = UpdateValidatorListBalance { stake_pool };
        // just do the validator list sequentially
        for (i, chunk) in validator_list_entries
            .chunks(MAX_VALIDATORS_TO_UPDATE_PER_TX)
            .enumerate()
        {
            if chunk
                .iter()
                .all(|vsi| vsi.last_update_epoch >= current_epoch)
                && ctrl != UpdateCtrl::ForceAll
            {
                continue;
            }
            let start_index = i * MAX_VALIDATORS_TO_UPDATE_PER_TX;
            let ixs = vec![uvlb
                .full_ix_from_validator_slice(
                    program_id,
                    chunk,
                    UpdateValidatorListBalanceIxArgs {
                        start_index: start_index.try_into().unwrap(),
                        no_merge: false,
                    },
                )
                .unwrap()];
            let ixs = match send_mode {
                TxSendMode::DumpMsg => ixs,
                _ => with_auto_cb_ixs(rpc, &payer.pubkey(), ixs, &[], fee_limit_cb).await,
            };
            eprintln!(
                "Updating validator list [{}..{}]",
                start_index,
                std::cmp::min(
                    start_index + MAX_VALIDATORS_TO_UPDATE_PER_TX,
                    validator_list_entries.len()
                )
            );
            handle_tx_full(rpc, send_mode, &ixs, &[], &mut [payer]).await;
        }
    }

    // Update pool:
    let final_ixs = vec![
        update_stake_pool_balance_ix_with_program_id(
            program_id,
            UpdateStakePoolBalance { stake_pool }
                .resolve_for_prog(&program_id)
                .unwrap(),
        )
        .unwrap(),
        cleanup_removed_validator_entries_ix_with_program_id(
            program_id,
            CleanupRemovedValidatorEntries { stake_pool }
                .resolve()
                .unwrap(),
        )
        .unwrap(),
    ];
    let final_ixs = match send_mode {
        TxSendMode::DumpMsg => final_ixs,
        _ => with_auto_cb_ixs(rpc, &payer.pubkey(), final_ixs, &[], fee_limit_cb).await,
    };
    eprintln!("Sending final update tx");
    handle_tx_full(rpc, send_mode, &final_ixs, &[], &mut [payer]).await;
}

#[cfg(test)]
mod tests {
    use borsh::BorshSerialize;
    use sanctum_solana_test_utils::assert_tx_with_cb_ixs_within_size_limits;
    use sanctum_spl_stake_pool_lib::{account_resolvers::UpdateValidatorListBalance, ZERO_FEE};
    use solana_readonly_account::ReadonlyAccountData;
    use solana_sdk::pubkey::Pubkey;
    use spl_stake_pool_interface::{
        AccountType, FutureEpochFee, Lockup, StakePool, StakeStatus,
        UpdateValidatorListBalanceIxArgs,
    };

    use super::*;

    struct AccountData(pub Vec<u8>);

    impl ReadonlyAccountData for AccountData {
        type SliceDeref<'s>
            = Vec<u8>
        where
            Self: 's;

        type DataDeref<'d>
            = &'d Vec<u8>
        where
            Self: 'd;

        fn data(&self) -> Self::DataDeref<'_> {
            &self.0
        }
    }

    #[test]
    fn check_max_validators_to_update_ix_per_tx_limit() {
        let program_id = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let sp = StakePool {
            validator_list: Pubkey::new_unique(),
            reserve_stake: Pubkey::new_unique(),
            // dont cares:
            account_type: AccountType::StakePool,
            manager: Pubkey::new_unique(),
            staker: Pubkey::new_unique(),
            stake_deposit_authority: Pubkey::new_unique(),
            manager_fee_account: Pubkey::new_unique(),
            epoch_fee: ZERO_FEE,
            next_epoch_fee: FutureEpochFee::None,
            stake_deposit_fee: ZERO_FEE,
            stake_withdrawal_fee: ZERO_FEE,
            next_stake_withdrawal_fee: FutureEpochFee::None,
            stake_referral_fee: 0,
            sol_deposit_authority: None,
            sol_deposit_fee: ZERO_FEE,
            sol_referral_fee: 0,
            sol_withdraw_authority: None,
            sol_withdrawal_fee: ZERO_FEE,
            next_sol_withdrawal_fee: FutureEpochFee::None,
            token_program: Pubkey::new_unique(),
            pool_mint: Pubkey::new_unique(),
            preferred_deposit_validator_vote_address: None,
            preferred_withdraw_validator_vote_address: None,
            lockup: Lockup {
                unix_timestamp: 0,
                epoch: 0,
                custodian: Pubkey::default(),
            },
            total_lamports: 0,
            pool_token_supply: 0,
            last_update_epoch: 0,
            stake_withdraw_bump_seed: 255,
            last_epoch_pool_token_supply: 0,
            last_epoch_total_lamports: 0,
        };
        // TODO: serializing here then deserializing again is dumb,
        // should update sdk to work with deserialized structs, not just raw accounts
        let stake_pool = Keyed {
            pubkey: pool,
            account: AccountData(sp.try_to_vec().unwrap()),
        };
        let vsi_list: Vec<ValidatorStakeInfo> = (0..MAX_VALIDATORS_TO_UPDATE_PER_TX)
            .map(|_| ValidatorStakeInfo {
                active_stake_lamports: 0,
                transient_stake_lamports: 0,
                last_update_epoch: 0,
                transient_seed_suffix: 0,
                unused: 0,
                validator_seed_suffix: 0,
                status: StakeStatus::Active,
                vote_account_address: Pubkey::new_unique(),
            })
            .collect();

        let ix = UpdateValidatorListBalance { stake_pool }
            .full_ix_from_validator_slice(
                program_id,
                &vsi_list,
                UpdateValidatorListBalanceIxArgs {
                    start_index: 0,
                    no_merge: false,
                },
            )
            .unwrap();
        // size = 1186
        assert_tx_with_cb_ixs_within_size_limits(&Pubkey::new_unique(), [ix].into_iter(), &[]);
    }
}
