use clap::Args;
use sanctum_solana_cli_utils::parse_pubkey_src;
use spl_stake_pool_interface::{StakePool, ValidatorList};

use crate::pool_config::{ConfigRaw, ConfigTomlFile};

use super::Subcmd;

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
    pub async fn run(args: crate::Args) {
        let Self { verbose, pool } = match args.subcmd {
            Subcmd::List(a) => a,
            _ => unreachable!(),
        };

        let pool = parse_pubkey_src(&pool).unwrap().pubkey();
        let rpc = args.config.nonblocking_rpc_client();
        let program_id = args.program.program_id();

        let mut display = ConfigRaw::default();
        display.set_pool_pk(pool);

        let fetched_pool = rpc.get_account(&pool).await.unwrap();
        if program_id != fetched_pool.owner {
            panic!(
                "Wrong stake pool program. Expected {}, but stake pool's is {}",
                args.program, fetched_pool.owner
            )
        }
        let decoded_pool =
            <StakePool as borsh::BorshDeserialize>::deserialize(&mut fetched_pool.data.as_ref())
                .unwrap();
        let validator_list_pk = decoded_pool.validator_list;
        display.set_pool(&program_id, pool, &decoded_pool);

        if verbose {
            let fetched_validator_list_data =
                rpc.get_account_data(&validator_list_pk).await.unwrap();
            let decoded_validator_list = <ValidatorList as borsh::BorshDeserialize>::deserialize(
                &mut fetched_validator_list_data.as_ref(),
            )
            .unwrap();
            display.set_validator_list(&program_id, &pool, &decoded_validator_list);
        }

        println!("{}", ConfigTomlFile { pool: &display })
    }
}
