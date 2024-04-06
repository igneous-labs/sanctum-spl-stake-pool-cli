use std::str::FromStr;

use clap::{
    builder::{StringValueParser, TypedValueParser},
    Args,
};
use solana_sdk::pubkey::Pubkey;

#[derive(Args, Debug)]
#[command(long_about = "Deposit SOL into a stake pool")]
pub struct DepositSolArgs {
    #[arg(
        long,
        short,
        help = "System account to send SOL from. Defaults to config wallet if not set"
    )]
    pub from: Option<String>,

    #[arg(
        long,
        short,
        help = "Token account to receive the minted pool tokens. Defaults to from's ATA, optionally creating it, if not set."
    )]
    pub receive: Option<String>,

    #[arg(
        help = "Pubkey of the pool to deposit SOL into",
        value_parser = StringValueParser::new().try_map(|s| Pubkey::from_str(&s)),
    )]
    pub pool: Pubkey,

    #[arg(help = "Amount in SOL to deposit")]
    pub sol: f64,
}

impl DepositSolArgs {
    pub async fn run(_args: crate::Args) {
        /*
        let Self { pool_config } = match args.subcmd {
            Subcmd::CreatePool(a) => a,
            _ => unreachable!(),
        };
        */
    }
}
