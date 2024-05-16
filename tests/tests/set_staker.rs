use borsh::BorshDeserialize;
use sanctum_solana_test_utils::{
    cli::{assert_all_txs_success_nonempty, ExtendedCommand},
    test_fixtures_dir, ExtendedBanksClient, ExtendedProgramTest,
};
use sanctum_spl_stake_pool_cli::ConfigRaw;
use sanctum_spl_stake_pool_lib::{FindWithdrawAuthority, ZERO_FEE};
use solana_program_test::ProgramTest;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use spl_stake_pool_interface::{
    AccountType, FutureEpochFee, Lockup, StakePool, ValidatorList, ValidatorListHeader,
};

use crate::common::{
    add_all_stake_pool_accounts, add_spl_stake_pool_prog, setup, tmp_config_file, PoolArgs,
    PoolKeys, SPL_STAKE_POOL_LAST_UPGRADE_EPOCH,
};

#[tokio::test(flavor = "multi_thread")]
async fn set_staker_payer_curr_staker() {
    let [curr_staker, new_staker] = [
        "example-staker-keypair.json",
        "example-new-staker-keypair.json",
    ]
    .map(|p| read_keypair_file(test_fixtures_dir().join(p)).unwrap());
    let PoolKeys {
        pool,
        validator_list,
        reserve,
        mint,
    } = PoolKeys::gen();

    let sp = StakePool {
        account_type: AccountType::StakePool,
        staker: curr_staker.pubkey(),
        validator_list,
        pool_mint: mint,
        reserve_stake: reserve,
        // dont cares
        token_program: spl_token_interface::ID,
        total_lamports: 10_000_000_000,
        pool_token_supply: 10_000_000_000,
        last_update_epoch: SPL_STAKE_POOL_LAST_UPGRADE_EPOCH,
        stake_withdraw_bump_seed: FindWithdrawAuthority { pool }
            .run_for_prog(&spl_stake_pool_interface::ID)
            .1,
        preferred_deposit_validator_vote_address: None,
        preferred_withdraw_validator_vote_address: None,
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
        validators: vec![],
    };
    let va = [];
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
    let pt = pt.add_system_account(curr_staker.pubkey(), 1_000_000_000);

    let cfg = ConfigRaw {
        pool: Some(pool.to_string()),
        old_staker: None,
        staker: Some(new_staker.pubkey().to_string()),
        ..Default::default()
    };

    let (mut cmd, _cfg, mut bc, _rbh) = setup(pt, &curr_staker).await;
    let cfg_file = tmp_config_file(&cfg);

    cmd.arg("set-staker").arg(cfg_file.path());

    let exec_res = cmd.exec_b64_txs(&mut bc).await;
    assert_all_txs_success_nonempty(&exec_res);

    let StakePool { staker, .. } =
        StakePool::deserialize(&mut bc.get_account_data(pool).await.as_slice()).unwrap();

    assert_eq!(staker, new_staker.pubkey());
}
