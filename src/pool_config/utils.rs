use solana_sdk::{pubkey::Pubkey, rent::Rent, stake::state::StakeStateV2};

pub fn pubkey_opt_display(pubkey_opt: &Option<Pubkey>) -> String {
    pubkey_opt.map_or_else(|| "None".to_owned(), |pk| pk.to_string())
}

/// Ported from sanctum-spl-stake-pool-lib due to change to min delegation
const fn min_delegation() -> u64 {
    1_000_000_000
}

/// Returns lamports required to be transferred from the pool's
/// reserve to create a new validator stake account to add a validator to the pool
///
/// Ported from sanctum-spl-stake-pool-lib due to change to min delegation
pub fn lamports_for_new_vsa(rent: &Rent) -> u64 {
    rent.minimum_balance(std::mem::size_of::<StakeStateV2>())
        .saturating_add(min_delegation())
}
