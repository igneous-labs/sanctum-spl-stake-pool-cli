use borsh::BorshDeserialize;
use sanctum_solana_test_utils::{test_fixtures_dir, ExtendedBanksClient, ExtendedProgramTest};
use sanctum_spl_stake_pool_cli::ConfigRaw;
use sanctum_spl_stake_pool_lib::{lamports_for_new_vsa, FindWithdrawAuthority, ZERO_FEE};
use solana_program_test::ProgramTest;
use solana_sdk::{pubkey::Pubkey, rent::Rent, signature::read_keypair_file, signer::Signer};
use spl_stake_pool_interface::{
    AccountType, FutureEpochFee, Lockup, StakePool, StakeStatus, ValidatorList,
    ValidatorListHeader, ValidatorStakeInfo,
};

use crate::common::{
    add_all_stake_pool_accounts, add_spl_stake_pool_prog, add_vote_accounts,
    assert_all_txs_success_nonempty, exec_b64_txs, setup, tmp_config_file, zeta_vote, PoolArgs,
    PoolKeys, TransientStakeAccountState, ValidatorArgs, SPL_STAKE_POOL_LAST_UPGRADE_EPOCH,
};

#[tokio::test(flavor = "multi_thread")]
async fn remove_only_active_validator() {
    let staker =
        read_keypair_file(test_fixtures_dir().join("example-manager-keypair.json")).unwrap();
    let PoolKeys {
        pool,
        validator_list,
        reserve,
        mint,
    } = PoolKeys::gen();

    let sp = StakePool {
        account_type: AccountType::StakePool,
        staker: staker.pubkey(),
        validator_list,
        pool_mint: mint,
        reserve_stake: reserve,
        token_program_id: spl_token_interface::ID,
        // vsa has some active stake in it
        total_lamports: 10_000_000_000,
        pool_token_supply: 10_000_000_000,
        last_update_epoch: SPL_STAKE_POOL_LAST_UPGRADE_EPOCH,
        stake_withdraw_bump_seed: FindWithdrawAuthority { pool }
            .run_for_prog(&spl_stake_pool_interface::ID)
            .1,
        // set to None so default cfg doesnt change it
        preferred_deposit_validator_vote_address: None,
        preferred_withdraw_validator_vote_address: None,
        // dont cares
        stake_deposit_authority: Pubkey::default(),
        manager_fee_account: Pubkey::default(),
        lockup: Lockup {
            unix_timestamp: 0,
            epoch: 0,
            custodian: Pubkey::default(),
        },
        epoch_fee: ZERO_FEE,
        next_epoch_fee: FutureEpochFee::None,
        stake_deposit_fee: ZERO_FEE,
        stake_withdrawal_fee: ZERO_FEE,
        next_stake_withdrawal_fee: FutureEpochFee::None,
        stake_referral_fee: 0,
        sol_deposit_authority: None,
        sol_deposit_fee: ZERO_FEE,
        sol_referral_fee: 0,
        sol_withdraw_authority: None,
        sol_withdrawal_fee: ZERO_FEE,
        next_sol_withdrawal_fee: FutureEpochFee::None,
        last_epoch_pool_token_supply: 0,
        last_epoch_total_lamports: 0,
        manager: Pubkey::default(),
    };
    let vl = ValidatorList {
        header: ValidatorListHeader {
            account_type: AccountType::ValidatorList,
            max_validators: 1,
        },
        validators: vec![ValidatorStakeInfo {
            active_stake_lamports: 5_000_000_000,
            transient_stake_lamports: 0,
            last_update_epoch: SPL_STAKE_POOL_LAST_UPGRADE_EPOCH,
            transient_seed_suffix: 0,
            unused: 0,
            validator_seed_suffix: 0,
            status: StakeStatus::Active,
            vote_account_address: zeta_vote::ID,
        }],
    };
    let va = [ValidatorArgs {
        vsa_activation_epoch: SPL_STAKE_POOL_LAST_UPGRADE_EPOCH - 2,
        transient_state: TransientStakeAccountState::Activating, // dont care since 0
    }];
    let pt = add_all_stake_pool_accounts(
        ProgramTest::default(),
        PoolArgs {
            program: spl_stake_pool_interface::ID,
            pool,
            current_epoch: SPL_STAKE_POOL_LAST_UPGRADE_EPOCH,
        },
        &sp,
        &vl,
        &va,
    );
    let pt = add_spl_stake_pool_prog(pt);
    let pt = add_vote_accounts(pt);
    let pt = pt.add_system_account(staker.pubkey(), 1_000_000_000);

    let cfg = ConfigRaw {
        pool: Some(pool.to_string()),
        validators: None,
        ..Default::default()
    };

    let (mut cmd, _cfg, mut bc, _rbh) = setup(pt, &staker).await;
    let cfg_file = tmp_config_file(&cfg);

    cmd.arg("sync-validator-list").arg(cfg_file.path());

    let exec_res = exec_b64_txs(&mut cmd, &mut bc).await;
    assert_all_txs_success_nonempty(&exec_res);

    let ValidatorList { validators, .. } =
        ValidatorList::deserialize(&mut bc.get_account_data(validator_list).await.as_slice())
            .unwrap();

    assert_eq!(validators.len(), 1);
    let ValidatorStakeInfo {
        active_stake_lamports,
        status,
        ..
    } = &validators[0];
    assert_eq!(
        *active_stake_lamports,
        lamports_for_new_vsa(&Rent::default())
    );
    assert_eq!(*status, StakeStatus::DeactivatingAll);
}
