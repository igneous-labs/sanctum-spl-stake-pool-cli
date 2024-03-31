use clap::Parser;
use tokio::runtime::Runtime;

fn main() {
    let args = sanctum_spl_stake_pool_cli::Args::parse();
    let rt = Runtime::new().unwrap();
    rt.block_on(sanctum_spl_stake_pool_cli::Subcmd::run(args));
}
