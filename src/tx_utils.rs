use std::cmp::max;

use sanctum_solana_cli_utils::{TxSendMode, TxSendingNonblockingRpcClient};
use solana_client::{
    nonblocking::rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig,
    rpc_response::RpcSimulateTransactionResult,
};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    signer::Signer,
    transaction::VersionedTransaction,
};

use crate::sorted_signers::SortedSigners;

pub const MAX_ADD_VALIDATORS_IX_PER_TX: usize = 7;

const CU_BUFFER_RATIO: f64 = 1.15;

pub async fn with_auto_cb_ixs(
    rpc: &RpcClient,
    payer_pk: &Pubkey,
    mut ixs: Vec<Instruction>,
    luts: &[AddressLookupTableAccount],
    fee_limit_cu_lamports: u64,
) -> Vec<Instruction> {
    let message =
        VersionedMessage::V0(Message::try_compile(payer_pk, &ixs, luts, Hash::default()).unwrap());
    let tx_to_sim = VersionedTransaction {
        signatures: vec![Signature::default(); message.header().num_required_signatures.into()],
        message,
    };
    let RpcSimulateTransactionResult { units_consumed, .. } = rpc
        .simulate_transaction_with_config(
            &tx_to_sim,
            RpcSimulateTransactionConfig {
                sig_verify: false,
                replace_recent_blockhash: true, // must set to true or sim will error with blockhash not found
                commitment: None,
                encoding: None,
                accounts: None,
                min_context_slot: None,
            },
        )
        .await
        .unwrap()
        .value;
    let units = ((units_consumed.unwrap() as f64) * CU_BUFFER_RATIO).ceil();
    let lamport_per_cu = (fee_limit_cu_lamports as f64) / units;
    let microlamports_per_cu = (lamport_per_cu * 1_000_000.0).floor();
    let units = units as u32;
    let microlamports_per_cu = max(1, microlamports_per_cu as u64);
    ixs.insert(0, ComputeBudgetInstruction::set_compute_unit_limit(units));
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
    let rbh = rpc.get_latest_blockhash().await.unwrap();
    rpc.handle_tx(
        &VersionedTransaction::try_new(
            VersionedMessage::V0(Message::try_compile(&payer_pk, ixs, luts, rbh).unwrap()),
            &SortedSigners(signers),
        )
        .unwrap(),
        send_mode,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use solana_sdk::signature::Keypair;

    use crate::{
        pool_config::SyncValidatorListConfig, test_utils::assert_tx_with_cu_ixs_within_size_limits,
    };

    use super::*;

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
        };
        let (add, _remove) = svlc.changeset(&[]);
        let ixs = svlc.add_validators_ixs(add).unwrap();
        let mut iter = ixs.as_slice().chunks(MAX_ADD_VALIDATORS_IX_PER_TX);
        let add_validator_ix_chunk = iter.next().unwrap();
        assert_eq!(add_validator_ix_chunk.len(), MAX_ADD_VALIDATORS_IX_PER_TX);
        // size = 1231 WEW
        assert_tx_with_cu_ixs_within_size_limits(
            &payer.pubkey(),
            add_validator_ix_chunk.iter().cloned(),
        );
        assert!(iter.next().is_none());
    }
}
