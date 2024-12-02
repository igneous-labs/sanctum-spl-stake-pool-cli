use sanctum_solana_cli_utils::{
    HandleTxArgs, RecentBlockhash, TxSendMode, TxSendingNonblockingRpcClient,
};
use sanctum_solana_client_utils::{
    buffer_compute_units, calc_compute_unit_price, estimate_compute_unit_limit_nonblocking,
    to_est_cu_sim_tx, SortedSigners,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signer::Signer,
    transaction::VersionedTransaction,
};

pub const MAX_ADD_VALIDATORS_IX_PER_TX: usize = 7;

pub const MAX_REMOVE_VALIDATOR_IXS_ENUM_PER_TX: usize = 5;

pub const MAX_INCREASE_VALIDATOR_STAKE_IX_PER_TX: usize = 4;

const CU_BUFFER_RATIO: f64 = 1.1;

const CUS_REQUIRED_FOR_SET_CU_LIMIT_IXS: u32 = 300;

pub async fn with_auto_cb_ixs(
    rpc: &RpcClient,
    payer_pk: &Pubkey,
    mut ixs: Vec<Instruction>,
    luts: &[AddressLookupTableAccount],
    fee_limit_cb_lamports: u64,
) -> Vec<Instruction> {
    if fee_limit_cb_lamports == 0 {
        return ixs;
    }
    let tx_to_sim = to_est_cu_sim_tx(payer_pk, &ixs, luts).unwrap();
    let units_consumed = estimate_compute_unit_limit_nonblocking(rpc, &tx_to_sim)
        .await
        .unwrap();
    let units_consumed = buffer_compute_units(units_consumed, CU_BUFFER_RATIO)
        .saturating_add(CUS_REQUIRED_FOR_SET_CU_LIMIT_IXS);
    let microlamports_per_cu = calc_compute_unit_price(units_consumed, fee_limit_cb_lamports);
    ixs.insert(
        0,
        ComputeBudgetInstruction::set_compute_unit_limit(units_consumed),
    );
    ixs.insert(
        0,
        ComputeBudgetInstruction::set_compute_unit_price(microlamports_per_cu),
    );
    ixs
}

/// First signer in signers is transaction payer
pub async fn handle_tx_full(
    rpc: &RpcClient,
    send_mode: TxSendMode,
    ixs: &[Instruction],
    luts: &[AddressLookupTableAccount],
    signers: &mut [&dyn Signer],
) {
    let payer_pk = signers[0].pubkey();
    signers.sort_by_key(|s| s.pubkey());
    let RecentBlockhash { hash, .. } = rpc.get_confirmed_blockhash().await.unwrap();
    rpc.handle_tx(
        &VersionedTransaction::try_new(
            VersionedMessage::V0(Message::try_compile(&payer_pk, ixs, luts, hash).unwrap()),
            &SortedSigners(signers),
        )
        .unwrap(),
        send_mode,
        HandleTxArgs::cli_default(),
    )
    .await
    .unwrap();
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use sanctum_solana_test_utils::assert_tx_with_cb_ixs_within_size_limits;
    use solana_sdk::{
        rent::Rent,
        signature::Keypair,
        stake::{
            stake_flags::StakeFlags,
            state::{Delegation, Meta, Stake, StakeStateV2},
        },
    };
    use spl_stake_pool_interface::{StakeStatus, ValidatorStakeInfo};

    use crate::{pool_config::SyncValidatorListConfig, SyncDelegationConfig};

    use super::*;

    fn mock_all_vsas_active_itr() -> impl Iterator<Item = StakeStateV2> {
        std::iter::repeat(StakeStateV2::Stake(
            Meta::default(),
            Stake {
                delegation: Delegation {
                    deactivation_epoch: u64::MAX,
                    ..Default::default()
                },
                ..Default::default()
            },
            StakeFlags::default(),
        ))
    }

    #[test]
    fn check_max_add_validators_ix_per_tx_limit() {
        let validators: HashSet<Pubkey> = (0..MAX_ADD_VALIDATORS_IX_PER_TX)
            .map(|_| Pubkey::new_unique())
            .collect();
        let payer = Keypair::new();
        let staker = Keypair::new();
        let svlc = SyncValidatorListConfig {
            program_id: Pubkey::new_unique(),
            payer: &payer,
            staker: &staker,
            pool: Pubkey::new_unique(),
            validator_list: Pubkey::new_unique(),
            reserve: Pubkey::new_unique(),
            validators,
            // dont care
            preferred_deposit_validator: None,
            preferred_withdraw_validator: None,
            rent: &Rent::default(),
        };
        let (add, _remove) = svlc.add_remove_changeset(&[]);
        let ixs = svlc.add_validators_ixs(add).unwrap();
        let mut iter = ixs.as_slice().chunks(MAX_ADD_VALIDATORS_IX_PER_TX);
        let add_validator_ix_chunk = iter.next().unwrap();
        assert_eq!(add_validator_ix_chunk.len(), MAX_ADD_VALIDATORS_IX_PER_TX);
        // size = 1231 WEW
        assert_tx_with_cb_ixs_within_size_limits(
            &payer.pubkey(),
            add_validator_ix_chunk.iter().cloned(),
            &[],
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn check_max_remove_validator_ixs_enum_per_tx_limit() {
        let validators: Vec<ValidatorStakeInfo> = (0..MAX_REMOVE_VALIDATOR_IXS_ENUM_PER_TX)
            .map(|_| ValidatorStakeInfo {
                // worst-case: all validators need to have stake removed
                active_stake_lamports: 1_000_000_000,
                vote_account_address: Pubkey::new_unique(),
                // dont care
                transient_stake_lamports: 0,
                last_update_epoch: 0,
                transient_seed_suffix: 0,
                unused: 0,
                validator_seed_suffix: 0,
                status: StakeStatus::Active,
            })
            .collect();
        let payer = Keypair::new();
        let staker = Keypair::new();
        let svlc = SyncValidatorListConfig {
            program_id: Pubkey::new_unique(),
            payer: &payer,
            staker: &staker,
            pool: Pubkey::new_unique(),
            validator_list: Pubkey::new_unique(),
            reserve: Pubkey::new_unique(),
            validators: HashSet::new(),
            rent: &Rent::default(),
            // dont care
            preferred_deposit_validator: None,
            preferred_withdraw_validator: None,
        };
        let (_add, remove) = svlc.add_remove_changeset(&validators);
        let ixs = svlc
            .remove_validators_ixs(remove.zip(mock_all_vsas_active_itr()))
            .unwrap();
        assert_eq!(ixs.len(), MAX_REMOVE_VALIDATOR_IXS_ENUM_PER_TX * 2);
        // size = 1184
        assert_tx_with_cb_ixs_within_size_limits(&payer.pubkey(), ixs.into_iter(), &[]);
    }

    #[test]
    fn check_max_increase_validator_stake_ixs_per_tx_limit() {
        let validators: Vec<ValidatorStakeInfo> = (0..MAX_INCREASE_VALIDATOR_STAKE_IX_PER_TX)
            .map(|_| ValidatorStakeInfo {
                // worst-case: all validators need to have stake increased
                active_stake_lamports: 0,
                vote_account_address: Pubkey::new_unique(),
                status: StakeStatus::Active,
                // dont care
                transient_stake_lamports: 0,
                last_update_epoch: 0,
                transient_seed_suffix: 0,
                unused: 0,
                validator_seed_suffix: 0,
            })
            .collect();
        let payer = Keypair::new();
        let staker = Keypair::new();
        let sdc = SyncDelegationConfig {
            program_id: Pubkey::new_unique(),
            payer: &payer,
            staker: &staker,
            pool: Pubkey::new_unique(),
            validator_list: Pubkey::new_unique(),
            reserve: Pubkey::new_unique(),
            rent: Rent::default(),
            reserve_lamports: u64::MAX,
            curr_epoch: 0,
        };
        let mock_vsa_state =
            StakeStateV2::Stake(Default::default(), Default::default(), Default::default());
        let cs = sdc.changeset(
            validators
                .iter()
                .zip(std::iter::repeat((&mock_vsa_state, &None, 1_000_000_000)))
                .map(|(vsi, (vsa, tsa, target))| (vsi, vsa, tsa, target)),
        );
        let ixs: Vec<_> = sdc.sync_delegation_ixs(cs).collect();
        assert_eq!(ixs.len(), MAX_INCREASE_VALIDATOR_STAKE_IX_PER_TX);
        /*
        eprintln!(
            "{}",
            sanctum_solana_test_utils::tx_ser_size_with_cb_ixs(
                &payer.pubkey(),
                ixs.into_iter(),
                &[]
            )
        );
         */
        // size = 1188
        assert_tx_with_cb_ixs_within_size_limits(&payer.pubkey(), ixs.into_iter(), &[]);
    }
}
