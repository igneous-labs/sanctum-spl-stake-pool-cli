use std::path::PathBuf;

use borsh::BorshDeserialize;
use clap::{
    builder::{StringValueParser, TypedValueParser},
    Args,
};
use sanctum_solana_cli_utils::{PubkeySrc, TokenAmtOrAll, TokenAmtOrAllParser, TxSendMode};
use sanctum_spl_stake_pool_lib::{
    lamports_for_new_vsa, FindTransientStakeAccount, FindTransientStakeAccountArgs,
    FindValidatorStakeAccount,
};
use solana_sdk::{
    clock::Clock, instruction::Instruction, rent::Rent, stake::state::StakeStateV2, sysvar,
};
use spl_stake_pool_interface::{StakePool, ValidatorList};

use crate::{
    next_epoch_stake_and_transient_status, parse_signer_pubkey_none,
    pool_config::ConfigRaw,
    tx_utils::{handle_tx_full, with_auto_cb_ixs},
    SyncDelegationConfig,
};

use super::Subcmd;

#[derive(Args, Debug)]
#[command(
    long_about = "(Staker only) Increase the stake delegated to one of the validators in the stake pool"
)]
pub struct IncreaseValidatorStakeArgs {
    #[arg(help = "Path to pool config file")]
    pub pool_config: PathBuf,

    #[arg(help = "The validator vote account to increase stake to")]
    pub validator: String,

    #[arg(
        help = "Amount of SOL stake to increase by. Also accepts 'all'.",
        value_parser = StringValueParser::new().map(|s| TokenAmtOrAllParser::new(9).parse(&s).unwrap()),
    )]
    pub stake: TokenAmtOrAll,
}

impl IncreaseValidatorStakeArgs {
    pub async fn run(args: crate::Args) {
        let Self {
            pool_config,
            validator,
            stake,
        } = match args.subcmd {
            Subcmd::IncreaseValidatorStake(a) => a,
            _ => unreachable!(),
        };
        let validator = PubkeySrc::parse(&validator).unwrap().pubkey();

        let ConfigRaw { pool, staker, .. } = ConfigRaw::read_from_path(pool_config).unwrap();

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let staker = staker.map_or_else(|| None, |s| parse_signer_pubkey_none(&s).unwrap());
        let staker = staker
            .as_ref()
            .map_or_else(|| payer.as_ref(), |s| s.as_ref());

        let pool = PubkeySrc::parse(pool.as_ref().unwrap()).unwrap().pubkey();

        let mut fetched = rpc
            .get_multiple_accounts(&[pool, sysvar::clock::ID, sysvar::rent::ID])
            .await
            .unwrap();
        let rent = fetched.pop().unwrap().unwrap();
        let clock = fetched.pop().unwrap().unwrap();
        let stake_pool_acc = fetched.pop().unwrap().unwrap();
        let program_id = stake_pool_acc.owner;

        let rent: Rent = bincode::deserialize(&rent.data).unwrap();
        let Clock {
            epoch: curr_epoch, ..
        } = bincode::deserialize(&clock.data).unwrap();
        let stake_pool = StakePool::deserialize(&mut stake_pool_acc.data.as_slice()).unwrap();

        if staker.pubkey() != stake_pool.staker {
            panic!(
                "Wrong staker. Expecting {}, got {}",
                stake_pool.staker,
                staker.pubkey()
            );
        }

        let mut fetched = rpc
            .get_multiple_accounts(&[stake_pool.validator_list, stake_pool.reserve_stake])
            .await
            .unwrap();
        let reserve_acc = fetched.pop().unwrap().unwrap();
        let validator_list_acc = fetched.pop().unwrap().unwrap();

        let ValidatorList { validators, .. } =
            ValidatorList::deserialize(&mut validator_list_acc.data.as_slice()).unwrap();
        let vsi = validators
            .iter()
            .find(|vsi| vsi.vote_account_address == validator)
            .unwrap_or_else(|| panic!("Validator {validator} not part of pool"));

        let (vsa_pubkey, _bump) = FindValidatorStakeAccount {
            pool,
            vote: validator,
            seed: None,
        }
        .run_for_prog(&program_id);
        let (tsa_pubkey, _bump) = FindTransientStakeAccount::new(FindTransientStakeAccountArgs {
            pool,
            vote: validator,
            seed: vsi.transient_seed_suffix,
        })
        .run_for_prog(&program_id);

        let fetched = rpc
            .get_multiple_accounts(&[vsa_pubkey, tsa_pubkey])
            .await
            .unwrap();
        let [Some(vsa), tsa] = fetched.as_slice() else {
            panic!("No vsa found");
        };
        let tsa = tsa.as_ref().map_or_else(
            || None,
            |acc| {
                (acc.owner == solana_program::stake::program::ID)
                    .then(|| StakeStateV2::deserialize(&mut acc.data.as_slice()).unwrap())
            },
        );
        let vsa = StakeStateV2::deserialize(&mut vsa.data.as_slice()).unwrap();

        let sdc = SyncDelegationConfig {
            program_id,
            payer: payer.as_ref(),
            staker,
            pool,
            validator_list: stake_pool.validator_list,
            reserve: stake_pool.reserve_stake,
            reserve_lamports: reserve_acc.lamports,
            curr_epoch,
            rent,
        };

        let (next_epoch_stake, _tsa_status) =
            next_epoch_stake_and_transient_status(&vsa, &tsa, curr_epoch);
        let desired_stake = match stake {
            TokenAmtOrAll::All { .. } => {
                let reserve_lamports_available = reserve_acc
                    .lamports
                    .saturating_sub(2 * lamports_for_new_vsa(&rent));
                next_epoch_stake.saturating_add(reserve_lamports_available)
            }
            TokenAmtOrAll::Amt { amt, .. } => next_epoch_stake.saturating_add(amt),
        };
        let changes = sdc.changeset(std::iter::once((vsi, &vsa, &tsa, desired_stake)));
        changes.print_all_changes();

        // should only have 1 ix
        let ixs: Vec<Instruction> = sdc.sync_delegation_ixs(changes).collect();
        if !ixs.is_empty() {
            let ixs = match args.send_mode {
                TxSendMode::DumpMsg => ixs,
                _ => with_auto_cb_ixs(&rpc, &payer.pubkey(), ixs, &[], args.fee_limit_cb).await,
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &ixs,
                &[],
                &mut sdc.signers_maybe_dup(),
            )
            .await;
        }
    }
}
