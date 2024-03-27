use std::cmp::max;

use sanctum_solana_cli_utils::{TxSendMode, TxSendingNonblockingRpcClient};
use solana_client::{
    nonblocking::rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig,
    rpc_response::RpcSimulateTransactionResult,
};
use solana_sdk::{
    address_lookup_table::{state::AddressLookupTable, AddressLookupTableAccount},
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    signer::Signer,
    signers::Signers,
    transaction::VersionedTransaction,
};

use crate::consts::srlut;

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
                inner_instructions: true,
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
    mut signers: Vec<&dyn Signer>,
) {
    let payer_pk = signers[0].pubkey();
    signers.sort_by_key(|s| s.pubkey());
    let rbh = rpc.get_latest_blockhash().await.unwrap();
    rpc.handle_tx(
        &VersionedTransaction::try_new(
            VersionedMessage::V0(Message::try_compile(&payer_pk, ixs, luts, rbh).unwrap()),
            &SortedSigners(&signers),
        )
        .unwrap(),
        send_mode,
    )
    .await;
}

/// newtype to impl Signers on to avoid lifetime errors from Vec::dedup()
pub struct SortedSigners<'slice, 'signer>(pub &'slice [&'signer dyn Signer]);

impl<'slice, 'signer> SortedSigners<'slice, 'signer> {
    pub fn iter(&self) -> SortedSignerIter<'_, '_, '_> {
        SortedSignerIter {
            inner: self,
            curr_i: 0,
        }
    }
}

pub struct SortedSignerIter<'a, 'slice, 'signer> {
    inner: &'a SortedSigners<'slice, 'signer>,
    curr_i: usize,
}

impl<'a, 'slice, 'signer> Iterator for SortedSignerIter<'a, 'slice, 'signer> {
    type Item = &'a dyn Signer;

    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.inner.0.get(self.curr_i)?;
        let curr_pk = curr.pubkey();
        self.curr_i += 1;
        while let Some(next) = self.inner.0.get(self.curr_i) {
            if next.pubkey() != curr_pk {
                break;
            }
            self.curr_i += 1;
        }
        Some(*curr)
    }
}

impl<'slice, 'signer> Signers for SortedSigners<'slice, 'signer> {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|s| s.pubkey()).collect()
    }

    fn try_pubkeys(&self) -> Result<Vec<Pubkey>, solana_sdk::signer::SignerError> {
        self.iter().map(|s| s.try_pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter().map(|s| s.sign_message(message)).collect()
    }

    fn try_sign_message(
        &self,
        message: &[u8],
    ) -> Result<Vec<Signature>, solana_sdk::signer::SignerError> {
        self.iter().map(|s| s.try_sign_message(message)).collect()
    }

    fn is_interactive(&self) -> bool {
        self.iter().any(|s| s.is_interactive())
    }
}

pub async fn fetch_srlut(rpc: &RpcClient) -> AddressLookupTableAccount {
    let srlut = rpc.get_account(&srlut::ID).await.unwrap();
    AddressLookupTableAccount {
        key: srlut::ID,
        addresses: AddressLookupTable::deserialize(&srlut.data)
            .unwrap()
            .addresses
            .into(),
    }
}
