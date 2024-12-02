use std::{error::Error, fs::read_to_string, num::NonZeroU32, path::Path};

use sanctum_solana_cli_utils::PubkeySrc;
use sanctum_spl_stake_pool_lib::{
    FindTransientStakeAccount, FindTransientStakeAccountArgs, FindValidatorStakeAccount,
    FindValidatorStakeAccountArgs,
};
use serde::{Deserialize, Serialize};
use solana_readonly_account::{ReadonlyAccountLamports, ReadonlyAccountPubkey};
use solana_sdk::pubkey::Pubkey;
use spl_stake_pool_interface::{Fee, FutureEpochFee, StakeStatus, ValidatorStakeInfo};

use crate::SplStakePoolProgram;

/// Owned version of [`ConfigTomlFile`].
/// Used to deserialize input config toml files
#[derive(Debug, Deserialize, Serialize)]
struct ConfigTomlFileOwned {
    pub pool: ConfigRaw,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConfigRaw {
    pub program: Option<SplStakePoolProgram>,
    pub mint: Option<String>,
    pub token_program: Option<String>,
    pub pool: Option<String>,
    pub validator_list: Option<String>,
    pub manager: Option<String>,
    pub manager_fee_account: Option<String>,
    pub staker: Option<String>,
    pub stake_deposit_auth: Option<String>,
    pub stake_withdraw_auth: Option<String>, // fixed PDA, only displayed for info purposes
    pub sol_deposit_auth: Option<String>,
    pub sol_withdraw_auth: Option<String>,
    pub preferred_deposit_validator: Option<String>,
    pub preferred_withdraw_validator: Option<String>,
    pub max_validators: Option<u32>,
    pub stake_deposit_referral_fee: Option<u8>,
    pub sol_deposit_referral_fee: Option<u8>,
    pub epoch_fee: Option<Fee>,
    pub stake_withdrawal_fee: Option<Fee>,
    pub sol_withdrawal_fee: Option<Fee>,
    pub stake_deposit_fee: Option<Fee>,
    pub sol_deposit_fee: Option<Fee>,
    pub total_lamports: Option<u64>,
    pub pool_token_supply: Option<u64>,
    pub last_update_epoch: Option<u64>,
    pub next_epoch_fee: Option<FutureEpochFee>,
    pub next_stake_withdrawal_fee: Option<FutureEpochFee>,
    pub next_sol_withdrawal_fee: Option<FutureEpochFee>,
    pub last_epoch_pool_token_supply: Option<u64>,
    pub last_epoch_total_lamports: Option<u64>,
    pub old_manager: Option<String>, // only present for sync-pool when changing manager
    pub old_staker: Option<String>,  // only present for set-staker
    pub reserve: Option<ReserveConfigRaw>,
    pub validators: Option<Vec<ValidatorConfigRaw>>, // put this last so it gets outputted last in toml Serialize
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ValidatorConfigRaw {
    pub vote: String,
    pub active_stake_lamports: Option<u64>,
    pub transient_stake_lamports: Option<u64>,
    pub last_update_epoch: Option<u64>,
    pub validator_seed_suffix: Option<NonZeroU32>,
    pub transient_seed_suffix: Option<u64>,
    pub status: Option<StakeStatus>,
    pub validator_stake_account: Option<String>,
    pub transient_stake_account: Option<String>,
}

impl ValidatorConfigRaw {
    pub fn from_vsi_program_pool(
        ValidatorStakeInfo {
            active_stake_lamports,
            transient_stake_lamports,
            last_update_epoch,
            transient_seed_suffix,
            validator_seed_suffix,
            status,
            vote_account_address,
            ..
        }: &ValidatorStakeInfo,
        program_id: &Pubkey,
        pool: &Pubkey,
    ) -> Self {
        let validator_seed_suffix = NonZeroU32::new(*validator_seed_suffix);
        Self {
            vote: vote_account_address.to_string(),
            active_stake_lamports: Some(*active_stake_lamports),
            transient_stake_lamports: Some(*transient_stake_lamports),
            last_update_epoch: Some(*last_update_epoch),
            validator_seed_suffix,
            transient_seed_suffix: Some(*transient_seed_suffix),
            status: Some(status.clone()),
            validator_stake_account: Some(
                FindValidatorStakeAccount::new(FindValidatorStakeAccountArgs {
                    pool: *pool,
                    vote: *vote_account_address,
                    seed: validator_seed_suffix,
                })
                .run_for_prog(program_id)
                .0
                .to_string(),
            ),
            transient_stake_account: Some(
                FindTransientStakeAccount::new(FindTransientStakeAccountArgs {
                    pool: *pool,
                    vote: *vote_account_address,
                    seed: *transient_seed_suffix,
                })
                .run_for_prog(program_id)
                .0
                .to_string(),
            ),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ReserveConfigRaw {
    pub address: String,
    pub lamports: Option<u64>,
}

impl ReserveConfigRaw {
    pub fn from_pk(pk: &Pubkey) -> Self {
        Self {
            address: pk.to_string(),
            lamports: None,
        }
    }

    pub fn from_reserve_stake_acc<A: ReadonlyAccountLamports + ReadonlyAccountPubkey>(
        account: A,
    ) -> Self {
        let mut res = Self::from_pk(account.pubkey());
        res.lamports = Some(account.lamports());
        res
    }
}

impl ConfigRaw {
    pub fn read_from_path<P: AsRef<Path>>(path: P) -> Result<ConfigRaw, std::io::Error> {
        // toml crate only handles strings, not io::Read lol
        let s = read_to_string(path)?;
        let ConfigTomlFileOwned { pool } =
            toml::from_str(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(pool)
    }
}

/// Used to serialize output config tomls
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ConfigTomlFile<'a> {
    pub pool: &'a ConfigRaw,
}

impl<'a> std::fmt::Display for ConfigTomlFile<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&toml::to_string_pretty(self).unwrap())
    }
}

/// Used to deserialize input config toml files
#[derive(Debug, Deserialize, Serialize)]
struct SyncDelegationConfigTomlFile {
    pub pool: SyncDelegationConfigToml,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SyncDelegationConfigToml {
    pub pool: String,
    pub staker: Option<String>,
    pub validators: Option<Vec<ValidatorDelegationRaw>>, // put this last so it gets outputted last in toml Serialize
}

impl SyncDelegationConfigToml {
    pub fn read_from_path<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        // toml crate only handles strings, not io::Read lol
        let s = read_to_string(path)?;
        let SyncDelegationConfigTomlFile { pool } =
            toml::from_str(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(pool)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ValidatorDelegationRaw {
    pub vote: String,
    pub target: ValidatorDelegationTarget,
}

#[derive(Clone, Copy, Debug)]
pub struct ValidatorDelegation {
    pub vote: Pubkey,
    pub target: ValidatorDelegationTarget,
}

impl TryFrom<ValidatorDelegationRaw> for ValidatorDelegation {
    type Error = Box<dyn Error>;

    fn try_from(
        ValidatorDelegationRaw { vote, target }: ValidatorDelegationRaw,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            target,
            vote: PubkeySrc::parse(&vote)?.pubkey(),
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValidatorDelegationTarget {
    Lamports(u64),
    Remainder,
}

pub fn is_delegation_scheme_valid<'a>(
    targets: impl Iterator<Item = &'a ValidatorDelegationTarget>,
) -> Result<(), &'static str> {
    let mut has_remainder = false;
    for target in targets {
        if matches!(target, ValidatorDelegationTarget::Remainder) {
            if has_remainder {
                return Err("Can only have at most one validator with target=remainder");
            } else {
                has_remainder = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sanctum_solana_test_utils::test_fixtures_dir;
    use spl_stake_pool_interface::Fee;

    use super::*;

    #[test]
    fn deser_example_config() {
        let example_path = test_fixtures_dir().join("example-init-pool-config.toml");
        let res = ConfigRaw::read_from_path(example_path).unwrap();
        // sample some fields
        assert_eq!(res.max_validators, Some(2));
        assert_eq!(
            res.epoch_fee,
            Some(Fee {
                denominator: 100,
                numerator: 6
            })
        );
        assert_eq!(res.validators.as_ref().unwrap().len(), 2);
        assert_eq!(
            res.validators.as_ref().unwrap()[0].vote,
            "BLADE1qNA1uNjRgER6DtUFf7FU3c1TWLLdpPeEcKatZ2"
        );
        assert_eq!(
            res.validators.as_ref().unwrap()[1].vote,
            "FnAPJkzf19s87sm24Qhv6bHZMZvZ43gjNUBRgjwXpD4v"
        );

        eprintln!("{}", ConfigTomlFile { pool: &res });
    }

    #[test]
    fn deser_example_validator_delegation_config() {
        let example_path = test_fixtures_dir().join("example-sync-delegation-config.toml");
        let pool = SyncDelegationConfigToml::read_from_path(example_path).unwrap();
        let scheme = pool.validators.as_ref().unwrap();
        eprintln!("{scheme:#?}");
        is_delegation_scheme_valid(scheme.iter().map(|vdr| &vdr.target)).unwrap();
        eprintln!(
            "{}",
            toml::to_string_pretty(&SyncDelegationConfigTomlFile { pool }).unwrap()
        )
    }
}
