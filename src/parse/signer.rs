use std::{error::Error, str::FromStr};

use sanctum_solana_cli_utils::parse_signer;
use solana_sdk::{
    pubkey::Pubkey,
    signer::{null_signer::NullSigner, Signer},
};

/// Need to use macro here instead of fn because of differences in &dyn Signer reference scope
/// between $fallback and signer parsed from $arg.
///
/// Output of this macro is declare $arg as `&dyn Signer`
macro_rules! ps {
    // @fb is fallback_signer: &dyn Signer
    // @sm is args.send_mode: TxSendMode

    // with fallback
    (
        $arg:ident,
        @fb $fallback:expr,
        @sm $sm:expr
    ) => {
        ps!($arg, @sm $sm, @internal);
        let $arg = $arg.as_ref().map_or_else(|| $fallback, |s| s.as_ref());
    };

    // no fallback, simply unwrap and panic if None
    (
        $arg:ident,
        @sm $sm:expr
    ) => {
        ps!($arg, @sm $sm, @internal);
        let $arg = $arg.as_ref().unwrap();
    };

    (
        $arg:ident,
        @sm $sm:expr,
        @internal
    ) => {
        let $arg = match $sm {
            sanctum_solana_cli_utils::TxSendMode::DumpMsg
            | sanctum_solana_cli_utils::TxSendMode::SimOnly => $arg
                .as_ref()
                .map(|s| crate::parse_signer_allow_pubkey(s).unwrap()),
            sanctum_solana_cli_utils::TxSendMode::SendActual => $arg
                .as_ref()
                .map_or_else(|| None, |s| crate::parse_signer_pubkey_none(s).unwrap()),
        };
    };
}
pub(crate) use ps;

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

/// `NullSigner` is returned if `s` is a pubkey
pub fn parse_signer_allow_pubkey(s: &str) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    match Pubkey::from_str(s) {
        Ok(pk) => Ok(Box::new(NullSigner::new(&pk))),
        Err(_e) => parse_signer(s),
    }
}
