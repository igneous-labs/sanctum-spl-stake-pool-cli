use std::str::FromStr;

use clap::{
    builder::{StringValueParser, TypedValueParser, ValueParser},
    Parser,
};
use sanctum_solana_cli_utils::{ConfigWrapper, TxSendMode};
use solana_sdk::pubkey::Pubkey;
use subcmd::Subcmd;
use tokio::runtime::Runtime;

mod consts;
mod pool_config;
mod subcmd;
mod tx_utils;
mod utils;

#[cfg(test)]
mod test_utils;

#[derive(Parser, Debug)]
#[command(author, version, about = "Sanctum SPL Stake Pool CLI")]
pub struct Args {
    #[arg(
        long,
        short,
        help = "Path to solana CLI config. Defaults to solana cli default if not provided",
        default_value = "",
        value_parser = ValueParser::new(ConfigWrapper::parse_from_path)
    )]
    pub config: ConfigWrapper,

    #[arg(
        long,
        short,
        help = "Transaction send mode.
- send-actual: signs and sends the tx to the cluster specified in config and outputs hash to stderr
- sim-only: simulates the tx against the cluster and outputs logs to stderr
- dump-msg: dumps the base64 encoded tx to stdout. For use with inspectors and multisigs",
        default_value_t = TxSendMode::default(),
        value_enum,
    )]
    pub send_mode: TxSendMode,

    #[arg(
        long,
        short,
        help = "0 - disable ComputeBudgetInstruction prepending.
Any positive integer: enable dynamic CU calculation
- before sending a TX, simulate the tx and prepend with appropriate ComputeBudgetInstructions.
This arg is the max priority fee the user will pay per transaction in lamports.",
        default_value_t = 1
    )]
    pub fee_limit_cu: u64,

    #[arg(
        long,
        short,
        help = "program ID of the SPL stake pool program",
        default_value_t = spl_stake_pool_interface::ID,
        value_parser = StringValueParser::new().try_map(|s| Pubkey::from_str(&s)),
    )]
    pub program: Pubkey,

    #[command(subcommand)]
    pub subcmd: Subcmd,
}

fn main() {
    let args = Args::parse();
    let rt = Runtime::new().unwrap();
    rt.block_on(Subcmd::run(args));
}
