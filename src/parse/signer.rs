use std::{error::Error, str::FromStr};

use sanctum_solana_cli_utils::parse_signer;
use solana_sdk::{
    pubkey::Pubkey,
    signer::{null_signer::NullSigner, Signer},
};

/// Returns
/// - `Ok(None)` if `s` is a pubkey,
/// - `Ok(Some(signer))` if `s` is a valid signer e.g. keypair file path
///
/// This helps to ignore pubkeys in toml files created from view dumps and fallback to payer
///
/// ideally this goes into sanctum-solana-cli-utils
/// idk why [`solana_clap_utils::signer_from_path`] accepts pubkeys even though
/// default [`solana_clap_utils::SignerFromPathConfig`] sets allow_null_signer to false
pub fn parse_signer_pubkey_none(s: &str) -> Result<Option<Box<dyn Signer>>, Box<dyn Error>> {
    match Pubkey::from_str(s) {
        Ok(_pk) => Ok(None),
        Err(_) => parse_signer(s).map(Some),
    }
}

// Need to use macro here instead of fn because of differences in &dyn Signer scope
// between $payer and signer parsed from $arg_string_opt
macro_rules! parse_signer_fallback_payer {
    ($arg_string_opt:ident, $payer:expr) => {
        let $arg_string_opt = $arg_string_opt
            .as_ref()
            .map_or_else(|| None, |s| crate::parse_signer_pubkey_none(s).unwrap());
        let $arg_string_opt = $arg_string_opt
            .as_ref()
            .map_or_else(|| $payer.as_ref(), |s| s.as_ref());
    };
}
pub(crate) use parse_signer_fallback_payer;

/// `NullSigner` is returned if `s` is a pubkey
pub fn parse_signer_allow_pubkey(s: &str) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    match Pubkey::from_str(s) {
        Ok(pk) => Ok(Box::new(NullSigner::new(&pk))),
        Err(_e) => parse_signer(s),
    }
}
