use solana_sdk::{pubkey::Pubkey, signer::Signer};

use crate::ValidatorDelegation;

/// All generated ixs must be signed by staker only.
/// Increases and decreases additional validator stake to match the target values
#[derive(Debug)]
pub struct SyncDelegationConfig<'a> {
    pub program_id: Pubkey,
    pub payer: &'a (dyn Signer + 'static),
    pub staker: &'a (dyn Signer + 'static),
    pub target_delegations: &'a [ValidatorDelegation],
    pub pool: Pubkey,
    pub validator_list: Pubkey,
    pub reserve: Pubkey,
}

#[derive(Debug, Clone, Copy)]
pub enum SyncDelegationAction {
    Increase(u64),
    Decrease(u64),
}

/*
impl<'a> SyncDelegationConfig<'a> {
    pub fn actions<'me>(
        &'me self,
        validator_list: &'me [ValidatorStakeInfo],
    ) -> impl Iterator<Item = (&'me ValidatorStakeInfo, SyncDelegationAction)> + Clone {
        self.target_delegations.iter().filter_map(|vd| {})
    }
}
 */
