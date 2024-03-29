use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    transaction::VersionedTransaction,
};

pub const TX_SIZE_LIMIT: usize = 1232;

// TODO: move this to sanctum-solana-test-utils
pub fn assert_tx_with_cu_ixs_within_size_limits(
    payer: &Pubkey,
    ixs: impl Iterator<Item = Instruction>,
) {
    let mut final_ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(0),
        ComputeBudgetInstruction::set_compute_unit_price(0),
    ];
    final_ixs.extend(ixs);
    let message = VersionedMessage::V0(
        Message::try_compile(payer, &final_ixs, &[], Hash::default()).unwrap(),
    );
    let n_signers = message.header().num_required_signatures;

    let tx = VersionedTransaction {
        signatures: vec![Signature::default(); n_signers.into()],
        message,
    };
    let tx_len = bincode::serialize(&tx).unwrap().len();
    // println!("{tx_len}");
    assert!(tx_len < TX_SIZE_LIMIT);
}
