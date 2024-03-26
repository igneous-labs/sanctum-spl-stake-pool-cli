use std::{fs::read_to_string, path::Path};

use serde::Deserialize;
use spl_stake_pool_interface::Fee;

#[derive(Debug, Deserialize)]
struct ConfigFileTomlFile {
    pub pool: ConfigFileRaw,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConfigFileRaw {
    pub mint: Option<String>,
    pub pool_keypair: Option<String>,
    pub validator_list_keypair: Option<String>,
    pub reserve: Option<String>,
    pub manager: Option<String>,
    pub manager_fee_account: Option<String>,
    pub staker: Option<String>,
    pub deposit_auth: Option<String>,
    pub sol_deposit_auth: Option<String>,
    pub sol_withdraw_auth: Option<String>,
    pub preferred_deposit_validator: Option<String>,
    pub preferred_withdraw_validator: Option<String>,
    pub max_validators: Option<u32>,
    pub validators: Option<Vec<String>>,
    pub stake_deposit_referral_fee: Option<u8>,
    pub sol_deposit_referral_fee: Option<u8>,
    pub epoch_fee: Option<Fee>,
    pub stake_withdrawal_fee: Option<Fee>,
    pub sol_withdrawal_fee: Option<Fee>,
    pub stake_deposit_fee: Option<Fee>,
    pub sol_deposit_fee: Option<Fee>,
}

impl ConfigFileRaw {
    pub fn read_from_path<P: AsRef<Path>>(path: P) -> Result<ConfigFileRaw, std::io::Error> {
        // toml crate only handles strings, not io::Read lol
        let s = read_to_string(path)?;
        let ConfigFileTomlFile { pool } = toml::from_str(&s)?;
        Ok(pool)
    }
}

#[cfg(test)]
mod tests {
    use sanctum_solana_test_utils::test_fixtures_dir;
    use spl_stake_pool_interface::Fee;

    use super::ConfigFileRaw;

    #[test]
    fn deser_example_config() {
        let example_path = test_fixtures_dir().join("example-pool-config.toml");
        let res = ConfigFileRaw::read_from_path(example_path).unwrap();
        // sample some fields
        assert_eq!(res.max_validators, Some(10));
        assert_eq!(
            res.epoch_fee,
            Some(Fee {
                denominator: 100,
                numerator: 6
            })
        )
    }
}
