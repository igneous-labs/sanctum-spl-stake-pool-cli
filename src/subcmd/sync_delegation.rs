use std::{path::PathBuf, str::FromStr};

use borsh::BorshDeserialize;
use clap::Args;
use sanctum_solana_cli_utils::{parse_signer, PubkeySrc, TxSendMode};
use sanctum_spl_stake_pool_lib::account_resolvers::{
    AdditionalValidatorStakeSeeds, IncreaseAdditionalValidatorStake, ProgramIdAndVote,
};
use solana_readonly_account::keyed::Keyed;
use solana_sdk::pubkey::Pubkey;
use spl_stake_pool_interface::{
    increase_additional_validator_stake_ix_with_program_id, AdditionalValidatorStakeArgs,
    IncreaseAdditionalValidatorStakeIxArgs, StakePool,
};

use crate::{handle_tx_full, with_auto_cb_ixs, Subcmd, SyncDelegationConfigToml};

#[derive(Args, Debug)]
#[command(long_about = "(Staker only) sync target stake delegation amounts")]
pub struct SyncDelegationArgs {
    #[arg(help = "Path to sync delegation config file")]
    pub sync_delegation_config: PathBuf,
}

impl SyncDelegationArgs {
    pub async fn run(args: crate::Args) {
        let Self {
            sync_delegation_config,
        } = match args.subcmd {
            Subcmd::SyncDelegation(a) => a,
            _ => unreachable!(),
        };

        let SyncDelegationConfigToml {
            pool,
            staker,
            validators: _, // TODO
        } = SyncDelegationConfigToml::read_from_path(sync_delegation_config).unwrap();

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let pool = PubkeySrc::parse(&pool).unwrap().pubkey();

        let staker = staker
            .as_ref()
            .map_or_else(|| None, |s| parse_signer(s).ok());
        let staker = staker
            .as_ref()
            .map_or_else(|| payer.as_ref(), |s| s.as_ref());

        let fetched_pool = rpc.get_account(&pool).await.unwrap();
        let program_id = fetched_pool.owner;
        let stake_pool: StakePool =
            StakePool::deserialize(&mut fetched_pool.data.as_slice()).unwrap();

        if staker.pubkey() != stake_pool.staker {
            panic!(
                "Wrong staker. Expecting {}, got {}",
                stake_pool.staker,
                staker.pubkey()
            );
        }

        // TODO: DELETEME, this is just temp for compass
        let ix = increase_additional_validator_stake_ix_with_program_id(
            program_id,
            IncreaseAdditionalValidatorStake {
                stake_pool: Keyed {
                    pubkey: pool,
                    account: fetched_pool,
                },
            }
            .resolve_for_prog_with_seeds(
                ProgramIdAndVote {
                    program_id,
                    vote_account: Pubkey::from_str("11BPTRia5mJKh9JfS6fRwL7VKQNViqrbvZ8XrEQsB5n")
                        .unwrap(),
                },
                AdditionalValidatorStakeSeeds {
                    validator: None,
                    transient: 0,
                    ephemeral: 0,
                },
            )
            .unwrap(),
            IncreaseAdditionalValidatorStakeIxArgs {
                args: AdditionalValidatorStakeArgs {
                    lamports: 14_700_000_000,
                    transient_stake_seed: 0,
                    ephemeral_stake_seed: 0,
                },
            },
        )
        .unwrap();

        let ixs = vec![ix];
        let ixs = match args.send_mode {
            TxSendMode::DumpMsg => ixs,
            _ => with_auto_cb_ixs(&rpc, &payer.pubkey(), ixs, &[], args.fee_limit_cb).await,
        };
        handle_tx_full(
            &rpc,
            args.send_mode,
            &ixs,
            &[],
            &mut [payer.as_ref(), staker],
        )
        .await;
    }
}
