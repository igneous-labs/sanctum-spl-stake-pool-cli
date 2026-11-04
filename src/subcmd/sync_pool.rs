use std::path::PathBuf;

use borsh::BorshDeserialize;
use clap::Args;
use sanctum_solana_cli_utils::{PubkeySrc, TxSendMode};
use spl_stake_pool_interface::StakePool;

use crate::{
    pool_config::{ConfigRaw, SyncPoolConfig},
    ps,
    tx_utils::{handle_tx_full, with_auto_cb_ixs},
};

use super::Subcmd;

#[derive(Args, Debug)]
#[command(long_about = "(Manager only) sync stake pool settings with a pool config file")]
pub struct SyncPoolArgs {
    #[arg(
        help = r#"Path to pool config file to update the stake pool's settings to.
If `old-manager` is not set in the toml, it falls back to `manager`, then to the config wallet.
Make sure to set `old-manager` field in toml for a non-wallet current pool manager signer that differs from `manager`."#
    )]
    pub pool_config: PathBuf,
}

macro_rules! det_managers {
    (@old $old:ident, @new $new:ident, @manager $manager:ident, @payer $payer:expr, @sm $sm:expr) => {
        let $new = &$manager;
        ps!($manager, @fb $payer, @sm $sm);
        ps!($old, @fb $manager, @sm $sm);
        ps!($new, @fb $old, @sm $sm);
    };
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

        det_managers!(
            @old old_manager,
            @new new_manager,
            @manager manager,
            @payer payer.as_ref(),
            @sm args.send_mode
        );

        if old_manager.pubkey() != stake_pool.manager {
            panic!(
                "Wrong manager. Expecting {}, got {}",
                stake_pool.manager,
                old_manager.pubkey()
            );
        }

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

#[cfg(test)]
mod tests {
    use solana_sdk::{signature::NullSigner, signer::Signer};

    use super::*;

    #[test]
    fn det_managers_truth_table() {
        const OLD: &str = "xFmWKsXGCXJXjCia391Qa7bfo5Mue3keMHzdmWX8quL";
        const NEW: &str = "3VikFCvkef9k8mCLk3EqKqbAxNTP1vvM3UddT6kb6H84";
        const PAYER: &str = "ChABYn2UHMqDHz96ckQVPvsBvSWDmAq7HB4rt58XTjdZ";

        let [old_pk, new_pk, payer_pk] = [OLD, NEW, PAYER].map(|s| s.parse().unwrap());
        let payer: &dyn Signer = &NullSigner::new(&payer_pk);

        for ([expected_old, expected_new], [old, manager]) in [
            ([payer_pk, payer_pk], [None, None]),
            ([old_pk, old_pk], [Some(OLD), None]),
            ([new_pk, new_pk], [None, Some(NEW)]),
            ([old_pk, new_pk], [Some(OLD), Some(NEW)]),
        ] {
            det_managers!(
                @old old,
                @new new,
                @manager manager,
                @payer payer,
                @sm TxSendMode::DumpMsg
            );
            assert_eq!(expected_old, old.pubkey());
            assert_eq!(expected_new, new.pubkey());
        }
    }
}
