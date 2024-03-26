use clap::Args;

#[derive(Args, Debug)]
#[command(long_about = "List current pool info")]
pub struct ListArgs {
    #[arg(
        long,
        short,
        default_value_t = false,
        help = "Also display validator list info for the stake pool"
    )]
    pub verbose: bool,

    #[arg(
        help = "Address of the stake pool. Can either be a base58-encoded pubkey or keypair file"
    )]
    pub pool: String,
}

impl ListArgs {
    pub async fn run(_args: crate::Args) {
        todo!()
    }
}
