use std::path::PathBuf;

use borsh::BorshDeserialize;
use clap::Args;
use sanctum_solana_cli_utils::{PubkeySrc, TxSendMode};
use spl_stake_pool_interface::StakePool;

use crate::{
    parse_signer_allow_pubkey, parse_signer_pubkey_none_fallback,
    pool_config::{ConfigRaw, SyncPoolConfig},
    tx_utils::{handle_tx_full, with_auto_cb_ixs},
};

use super::Subcmd;

#[derive(Args, Debug)]
#[command(long_about = "(Manager only) sync stake pool settings with a pool config file")]
pub struct SyncPoolArgs {
    #[arg(help = "Path to pool config file to update the stake pool's settings to")]
    pub pool_config: PathBuf,
}

impl SyncPoolArgs {
    pub async fn run(args: crate::Args) {
        let Self { pool_config } = match args.subcmd {
            Subcmd::SyncPool(a) => a,
            _ => unreachable!(),
        };

        let ConfigRaw {
            pool,
            manager,
            manager_fee_account,
            staker,
            stake_deposit_auth,
            stake_deposit_referral_fee,
            sol_deposit_referral_fee,
            epoch_fee,
            stake_withdrawal_fee,
            sol_withdrawal_fee,
            stake_deposit_fee,
            sol_deposit_fee,
            sol_deposit_auth,
            sol_withdraw_auth,
            old_manager,
            ..
        } = ConfigRaw::read_from_path(pool_config).unwrap();

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let pool = PubkeySrc::parse(pool.as_ref().unwrap()).unwrap().pubkey();

        let fetched_pool = rpc.get_account(&pool).await.unwrap();
        let program_id = fetched_pool.owner;
        let stake_pool: StakePool =
            StakePool::deserialize(&mut fetched_pool.data.as_slice()).unwrap();

        parse_signer_pubkey_none_fallback!(old_manager, payer);

        if old_manager.pubkey() != stake_pool.manager {
            panic!(
                "Wrong manager. Expecting {}, got {}",
                stake_pool.manager,
                old_manager.pubkey()
            );
        }
        // allow pubkey signers to work with multisig programs
        let new_manager = manager
            .as_ref()
            .map(|s| parse_signer_allow_pubkey(s).unwrap());
        let new_manager = new_manager.as_ref().map_or_else(
            || old_manager,
            |nm| {
                if nm.pubkey() == old_manager.pubkey() {
                    old_manager
                } else {
                    nm.as_ref()
                }
            },
        );

        let [manager_fee_account, staker] = [
            (manager_fee_account, stake_pool.manager_fee_account),
            (staker, stake_pool.staker),
        ]
        .map(|(file_opt, stake_pool_val)| {
            file_opt.map_or_else(
                || stake_pool_val,
                |s| PubkeySrc::parse(&s).unwrap().pubkey(),
            )
        });

        let [sol_deposit_auth, sol_withdraw_auth, stake_deposit_auth] =
            [sol_deposit_auth, sol_withdraw_auth, stake_deposit_auth]
                .map(|string_opt| string_opt.map(|s| PubkeySrc::parse(&s).unwrap().pubkey()));

        let [sol_deposit_referral_fee, stake_deposit_referral_fee] = [
            (sol_deposit_referral_fee, stake_pool.sol_referral_fee),
            (stake_deposit_referral_fee, stake_pool.stake_referral_fee),
        ]
        .map(|(opt, stake_pool_val)| opt.unwrap_or(stake_pool_val));

        let [epoch_fee, stake_withdrawal_fee, sol_withdrawal_fee, stake_deposit_fee, sol_deposit_fee] =
            [
                (epoch_fee, &stake_pool.epoch_fee),
                (stake_withdrawal_fee, &stake_pool.stake_withdrawal_fee),
                (sol_withdrawal_fee, &stake_pool.sol_withdrawal_fee),
                (stake_deposit_fee, &stake_pool.stake_deposit_fee),
                (sol_deposit_fee, &stake_pool.sol_deposit_fee),
            ]
            .map(|(opt, stake_pool_val)| opt.unwrap_or(stake_pool_val.clone()));

        let spc = SyncPoolConfig {
            program_id,
            pool,
            payer: payer.as_ref(),
            manager: old_manager,
            new_manager,
            staker,
            manager_fee_account,
            sol_deposit_auth,
            stake_deposit_auth,
            sol_withdraw_auth,
            epoch_fee,
            stake_deposit_referral_fee,
            sol_deposit_referral_fee,
            stake_withdrawal_fee,
            sol_withdrawal_fee,
            stake_deposit_fee,
            sol_deposit_fee,
        };

        let changeset = spc.changeset(&stake_pool);
        for change in changeset.iter() {
            eprintln!("{change}");
        }
        if changeset.is_empty() {
            eprintln!("No changes necessary");
            return;
        }
        let sync_pool_ixs = spc.changeset_ixs(&changeset).unwrap();
        let sync_pool_ixs = match args.send_mode {
            TxSendMode::DumpMsg => sync_pool_ixs,
            _ => {
                with_auto_cb_ixs(&rpc, &payer.pubkey(), sync_pool_ixs, &[], args.fee_limit_cb).await
            }
        };
        handle_tx_full(
            &rpc,
            args.send_mode,
            &sync_pool_ixs,
            &[],
            &mut spc.signers_maybe_dup(),
        )
        .await;
    }
}
