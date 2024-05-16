use std::path::PathBuf;

use borsh::BorshDeserialize;
use clap::Args;
use sanctum_solana_cli_utils::{parse_signer, PubkeySrc, TxSendMode};
use spl_stake_pool_interface::{set_staker_ix_with_program_id, SetStakerKeys, StakePool};

use crate::{handle_tx_full, with_auto_cb_ixs, ConfigRaw, Subcmd};

#[derive(Args, Debug)]
#[command(long_about = "(Staker only) set a new staker from a pool config file")]
pub struct SetStakerArgs {
    #[arg(help = "Path to pool config file that contains the new staker to set to")]
    pub pool_config: PathBuf,
}

impl SetStakerArgs {
    pub async fn run(args: crate::Args) {
        let Self { pool_config } = match args.subcmd {
            Subcmd::SetStaker(a) => a,
            _ => unreachable!(),
        };

        let ConfigRaw {
            pool,
            staker,
            old_staker,
            ..
        } = ConfigRaw::read_from_path(pool_config).unwrap();

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let pool = PubkeySrc::parse(pool.as_ref().unwrap()).unwrap().pubkey();

        let curr_staker = old_staker
            .as_ref()
            .map_or_else(|| None, |s| parse_signer(s).ok());
        let curr_staker = curr_staker
            .as_ref()
            .map_or_else(|| payer.as_ref(), |c| c.as_ref()); // if old-staker is not a valid signer, treat it as None and fall back to payer

        let new_staker = staker.map_or_else(
            || payer.pubkey(),
            // unwrap to make sure provided input is valid
            |s| PubkeySrc::parse(&s).unwrap().pubkey(),
        );

        if curr_staker.pubkey() == new_staker {
            eprintln!("Curr staker already {new_staker}, no changes necessary");
            return;
        }

        let fetched_pool = rpc.get_account(&pool).await.unwrap();
        let program_id = fetched_pool.owner;
        let stake_pool: StakePool =
            StakePool::deserialize(&mut fetched_pool.data.as_slice()).unwrap();

        if curr_staker.pubkey() != stake_pool.staker {
            panic!(
                "Wrong staker. Expecting {}, got {}",
                stake_pool.staker,
                curr_staker.pubkey()
            );
        }

        let ixs = vec![set_staker_ix_with_program_id(
            program_id,
            SetStakerKeys {
                stake_pool: pool,
                signer: curr_staker.pubkey(),
                new_staker,
            },
        )
        .unwrap()];
        let ixs = match args.send_mode {
            TxSendMode::DumpMsg => ixs,
            _ => with_auto_cb_ixs(&rpc, &payer.pubkey(), ixs, &[], args.fee_limit_cb).await,
        };
        handle_tx_full(
            &rpc,
            args.send_mode,
            &ixs,
            &[],
            &mut [payer.as_ref(), curr_staker],
        )
        .await;
    }
}
