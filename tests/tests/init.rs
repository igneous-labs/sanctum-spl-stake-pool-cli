use std::str::FromStr;

use borsh::BorshDeserialize;
use sanctum_associated_token_lib::FindAtaAddressArgs;
use sanctum_solana_test_utils::{test_fixtures_dir, ExtendedBanksClient};
use sanctum_spl_stake_pool_lib::FindDepositAuthority;
use solana_program_test::ProgramTest;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use spl_stake_pool_interface::{Fee, FutureEpochFee, StakePool};

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

    let (exec_res, stderr) = exec_b64_txs(&mut cmd, &mut bc).await;
    eprintln!("{stderr}");
    assert_all_txs_success_nonempty(&exec_res);

    // TODO: more assertions
    let stake_pool: StakePool =
        StakePool::deserialize(&mut bc.get_account_data(stake_pool_pk).await.as_slice()).unwrap();
    assert_eq!(stake_pool.validator_list, validator_list_pk);
    assert_eq!(stake_pool.manager, manager.pubkey());
    assert_eq!(
        stake_pool.manager_fee_account,
        FindAtaAddressArgs {
            wallet: manager.pubkey(),
            mint,
            token_program: spl_token::ID
        }
        .find_ata_address()
        .0
    );
    assert_eq!(stake_pool.staker, manager.pubkey());
    assert_eq!(stake_pool.pool_mint, mint);
    assert_eq!(
        stake_pool.stake_deposit_authority,
        FindDepositAuthority {
            pool: stake_pool_pk,
        }
        .run_for_prog(&spl_stake_pool_interface::ID)
        .0
    );
    let example_funding_auth =
        Pubkey::from_str("DAgQZufbVTGvJkDd3FhtcLPcmWXX7h5jzcePyVKCWZoL").unwrap();
    assert_eq!(
        stake_pool.sol_deposit_authority.unwrap(),
        example_funding_auth
    );
    assert_eq!(
        stake_pool.sol_withdraw_authority.unwrap(),
        example_funding_auth
    );
    assert_eq!(
        stake_pool.epoch_fee,
        Fee {
            denominator: 100,
            numerator: 6,
        }
    );
    assert_eq!(
        stake_pool.stake_deposit_fee,
        Fee {
            denominator: 0,
            numerator: 0,
        }
    );
    assert_eq!(
        stake_pool.stake_withdrawal_fee,
        Fee {
            denominator: 1000,
            numerator: 1,
        }
    );
    assert_eq!(
        stake_pool.sol_deposit_fee,
        Fee {
            denominator: 1000,
            numerator: 1,
        }
    );
    assert_eq!(
        stake_pool.next_sol_withdrawal_fee,
        FutureEpochFee::Two {
            fee: Fee {
                denominator: 0,
                numerator: 0,
            }
        }
    );
    assert_eq!(stake_pool.stake_referral_fee, 50);
    assert_eq!(stake_pool.sol_referral_fee, 0);
}
