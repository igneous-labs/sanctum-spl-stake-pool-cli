use std::collections::HashSet;

use solana_sdk::{pubkey::Pubkey, signer::Signer};

/// All generated ixs must be signed by staker only.
/// Adds and removes validators from the list to match `self.validators`
/// TODO: SyncDelegationConfig for staker to control delegation every epoch
#[derive(Debug)]
pub struct SyncValidatorListConfig<'a> {
    pub program_id: Pubkey,
    pub pool: Pubkey,
    pub payer: &'a (dyn Signer + 'static),
    pub staker: &'a (dyn Signer + 'static),
    pub validators: HashSet<Pubkey>,
}
