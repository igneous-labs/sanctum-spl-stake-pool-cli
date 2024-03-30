use std::{fmt::Display, str::FromStr};

use solana_sdk::pubkey::{ParsePubkeyError, Pubkey};

pub mod sanctum_spl_stake_pool_prog {
    sanctum_macros::declare_program_keys!("SP12tWFxD9oJsVWNavTTBZvMbA6gkAmxtVgxdqvyvhY", []);
}

pub mod sanctum_spl_multi_stake_pool_prog {
    sanctum_macros::declare_program_keys!("SPMBzsVUuoHA4Jm6KunbsotaahvVikZs1JyTW6iJvbn", []);
}

#[derive(Clone, Copy, Debug)]
pub enum SplStakePoolProgram {
    Unknown(Pubkey),
    Spl,
    SanctumSpl,
    SanctumSplMulti,
}

impl SplStakePoolProgram {
    pub const HELP_STR: &'static str = "The SPL stake pool program.
Can either be a base58-encoded program ID or one of the following known programs (case-insensitive):
- spl
- sanctum-spl
- sanctum-spl-multi
";

    pub const SPL_ID: &'static str = "spl";
    pub const SANCTUM_SPL_ID: &'static str = "sanctum-spl";
    pub const SANCTUM_SPL_MULTI_ID: &'static str = "sanctum-spl-multi";

    pub fn program_id(&self) -> Pubkey {
        match self {
            Self::Unknown(pk) => *pk,
            Self::Spl => spl_stake_pool_interface::ID,
            Self::SanctumSpl => sanctum_spl_stake_pool_prog::ID,
            Self::SanctumSplMulti => sanctum_spl_multi_stake_pool_prog::ID,
        }
    }

    pub fn parse(arg: &str) -> Result<Self, ParsePubkeyError> {
        let arg_lower_case = arg.to_lowercase();
        match arg_lower_case.as_str() {
            Self::SPL_ID => return Ok(Self::Spl),
            Self::SANCTUM_SPL_ID => return Ok(Self::SanctumSpl),
            Self::SANCTUM_SPL_MULTI_ID => return Ok(Self::SanctumSplMulti),
            _ => (),
        }
        let pk = Pubkey::from_str(arg)?;
        Ok(Self::from(pk))
    }
}

impl From<Pubkey> for SplStakePoolProgram {
    fn from(value: Pubkey) -> Self {
        match value {
            spl_stake_pool_interface::ID => Self::Spl,
            sanctum_spl_stake_pool_prog::ID => Self::SanctumSpl,
            sanctum_spl_multi_stake_pool_prog::ID => Self::SanctumSplMulti,
            _ => Self::Unknown(value),
        }
    }
}

impl Display for SplStakePoolProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spl => f.write_str(Self::SPL_ID),
            Self::SanctumSpl => f.write_str(Self::SANCTUM_SPL_ID),
            Self::SanctumSplMulti => f.write_str(Self::SANCTUM_SPL_MULTI_ID),
            Self::Unknown(pk) => write!(f, "{pk}"),
        }
    }
}
