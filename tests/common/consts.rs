pub const SPL_STAKE_POOL_LAST_UPGRADE_EPOCH: u64 = 551;
pub const SPL_STAKE_POOL_LAST_UPGRADE_SLOT: u64 = 238_419_616;

pub mod dummy_sol_deposit_auth {
    sanctum_macros::declare_program_keys!("DUMMYSo1DEPoS1TAUTH1111111111111111111111111", []);
}

pub mod dummy_sol_withdraw_auth {
    sanctum_macros::declare_program_keys!("DUMMYSo1W1THDRAWAUTH11111111111111111111111", []);
}

pub mod shinobi_vote {
    sanctum_macros::declare_program_keys!("BLADE1qNA1uNjRgER6DtUFf7FU3c1TWLLdpPeEcKatZ2", []);
}

pub mod zeta_vote {
    sanctum_macros::declare_program_keys!("FnAPJkzf19s87sm24Qhv6bHZMZvZ43gjNUBRgjwXpD4v", []);
}
