//! lib-bin split so that internal types e.g. ConfigFileRaw are available for integration tests

mod luts;
mod parse;
mod pool_config;
mod subcmd;
mod tx_utils;
mod update;

use clap::{builder::ValueParser, Parser};
pub use luts::*;
pub use parse::*;
pub use pool_config::*;
use sanctum_solana_cli_utils::{ConfigWrapper, TxSendMode};
pub use subcmd::*;
pub use tx_utils::*;
pub use update::*;

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
- dump-msg: dumps the base64 encoded tx to stdout. For use with inspectors and multisigs
",
        default_value_t = TxSendMode::default(),
        value_enum,
    )]
    pub send_mode: TxSendMode,

    #[arg(
        long,
        short,
        help = "0 - disable ComputeBudgetInstruction prepending.
Any positive integer - enable dynamic compute budget calculation:
Before sending a TX, simulate the tx and prepend with appropriate ComputeBudgetInstructions.
This arg is the max priority fee the user will pay per transaction in lamports.
",
        default_value_t = 1
    )]
    pub fee_limit_cb: u64,

    #[command(subcommand)]
    pub subcmd: Subcmd,
}
