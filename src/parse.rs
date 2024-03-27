use sanctum_solana_cli_utils::parse_signer;
use solana_sdk::{signature::Keypair, signer::Signer};

pub fn parse_signer_or_rando_kp<S: AsRef<str>>(path_opt: Option<S>) -> Box<dyn Signer> {
    match path_opt {
        None => Box::new(Keypair::new()),
        Some(p) => parse_signer(p.as_ref()).unwrap(),
    }
}
