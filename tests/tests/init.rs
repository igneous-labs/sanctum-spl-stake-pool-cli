use borsh::BorshDeserialize;
use sanctum_solana_test_utils::{test_fixtures_dir, ExtendedBanksClient};
use solana_program_test::ProgramTest;
use solana_sdk::{signature::read_keypair_file, signer::Signer};
use spl_stake_pool_interface::StakePool;

use crate::common::{assert_all_txs_success_nonempty, exec_b64_txs, setup_init_manager_payer};

#[tokio::test(flavor = "multi_thread")]
async fn init_basic_manager_payer_same() {
    let mint = read_keypair_file(test_fixtures_dir().join("example-pool-mint-keypair.json"))
        .unwrap()
        .pubkey();
    let manager =
        read_keypair_file(test_fixtures_dir().join("example-manager-keypair.json")).unwrap();
    let stake_pool_pk = read_keypair_file(test_fixtures_dir().join("example-pool-keypair.json"))
        .unwrap()
        .pubkey();
    let validator_list_pk =
        read_keypair_file(test_fixtures_dir().join("example-validator-list-keypair.json"))
            .unwrap()
            .pubkey();
    let (mut cmd, _cfg, mut bc, _rbh) =
        setup_init_manager_payer(ProgramTest::default(), mint, &manager).await;

    cmd.arg("create-pool")
        .arg(test_fixtures_dir().join("example-init-pool-config.toml"));

    /*
    let std::process::Output { stderr, stdout, .. } = cmd.output().unwrap();
    eprintln!("{}", std::str::from_utf8(&stderr).unwrap());
    eprintln!("{}", std::str::from_utf8(&stdout).unwrap());
     */

    let exec_res = exec_b64_txs(&mut cmd, &mut bc).await;
    assert_all_txs_success_nonempty(&exec_res);

    // TODO: more assertions
    let stake_pool: StakePool =
        StakePool::deserialize(&mut bc.get_account_data(stake_pool_pk).await.as_slice()).unwrap();
    assert_eq!(stake_pool.validator_list, validator_list_pk);
    assert_eq!(stake_pool.manager, manager.pubkey());
    assert_eq!(stake_pool.staker, manager.pubkey());
    assert_eq!(stake_pool.pool_mint, mint);
}
