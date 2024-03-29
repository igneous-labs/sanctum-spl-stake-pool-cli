use solana_sdk::pubkey::Pubkey;

pub fn filter_default_stake_deposit_auth(
    stake_deposit_auth: Pubkey,
    default_stake_deposit_auth: &Pubkey,
) -> Option<Pubkey> {
    if stake_deposit_auth == *default_stake_deposit_auth {
        None
    } else {
        Some(stake_deposit_auth)
    }
}
