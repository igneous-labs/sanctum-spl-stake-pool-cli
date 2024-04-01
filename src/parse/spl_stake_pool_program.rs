use std::{fmt::Display, str::FromStr};

use serde::{de::Visitor, Deserialize, Serialize};
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

    pub fn disp_string(&self) -> String {
        match self {
            Self::Spl => Self::SPL_ID.to_owned(),
            Self::SanctumSpl => Self::SANCTUM_SPL_ID.to_owned(),
            Self::SanctumSplMulti => Self::SANCTUM_SPL_MULTI_ID.to_owned(),
            Self::Unknown(pk) => pk.to_string(),
        }
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
        f.write_str(&self.disp_string())
    }
}

impl Serialize for SplStakePoolProgram {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.disp_string())
    }
}

struct SplStakePoolProgramVisitor;

impl Visitor<'_> for SplStakePoolProgramVisitor {
    type Value = SplStakePoolProgram;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(SplStakePoolProgram::HELP_STR)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        SplStakePoolProgram::parse(v).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for SplStakePoolProgram {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(SplStakePoolProgramVisitor)
    }
}
