use std::path::PathBuf;

use clap::Args;
use solana_sdk::pubkey::Pubkey;

use crate::{pool_config::CreateConfig, subcmd::Subcmd};

#[derive(Args, Debug)]
#[command(long_about = "Create a new stake pool")]
pub struct CreatePoolArgs {
    #[arg(help = "Path to create-pool config file")]
    pub pool_config: PathBuf,
}

impl CreatePoolArgs {
    pub async fn run(args: crate::Args) {
        let Self { pool_config } = match args.subcmd {
            Subcmd::CreatePool(a) => a,
            _ => unreachable!(),
        };

        /*
        let rpc = args.config.nonblocking_rpc_client();
        let signer = args.config.signer();

        let full = CreateConfig {
            mint: Pubkey::default(),
            pool_keypair: signer.as_ref(),
            validator_list_keypair: signer.as_ref(),
            manager: Pubkey::default(),
            reserve: signer.as_ref(),
        };

        rpc.get_epoch_info().await.unwrap();

        full.pool_keypair.sign_message(&[]);

        rpc.get_epoch_info().await.unwrap();
         */

        todo!()
    }
}
