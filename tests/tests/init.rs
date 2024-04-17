use borsh::BorshDeserialize;
use sanctum_associated_token_lib::FindAtaAddressArgs;
use sanctum_solana_test_utils::{
    cli::{assert_all_txs_success_nonempty, ExtendedCommand},
    test_fixtures_dir, ExtendedBanksClient,
};
use sanctum_spl_stake_pool_lib::FindDepositAuthority;
use solana_program_test::ProgramTest;
use solana_sdk::{signature::read_keypair_file, signer::Signer};
use spl_stake_pool_interface::{
    Fee, FutureEpochFee, StakePool, ValidatorList, ValidatorListHeader,
};

use crate::common::{
    dummy_sol_deposit_auth, dummy_sol_withdraw_auth, setup_init_manager_payer, shinobi_vote,
    zeta_vote,
};

#[tokio::test(flavor = "multi_thread")]
async fn init_basic_manager_payer_same() {
    const MAX_VALIDATORS: u32 = 2;

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

    let exec_res = cmd.exec_b64_txs(&mut bc).await;
    assert_all_txs_success_nonempty(&exec_res);

    let stake_pool: StakePool =
        StakePool::deserialize(&mut bc.get_account_data(stake_pool_pk).await.as_slice()).unwrap();
    assert_eq!(stake_pool.validator_list, validator_list_pk);
    assert_eq!(stake_pool.manager, manager.pubkey());
    assert_eq!(
        stake_pool.manager_fee_account,
        FindAtaAddressArgs {
            wallet: manager.pubkey(),
            mint,
            token_program: spl_token_interface::ID
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
    assert_eq!(
        stake_pool.sol_deposit_authority.unwrap(),
        dummy_sol_deposit_auth::ID
    );
    assert_eq!(
        stake_pool.sol_withdraw_authority.unwrap(),
        dummy_sol_withdraw_auth::ID
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
    assert_eq!(
        stake_pool.preferred_deposit_validator_vote_address,
        Some(shinobi_vote::ID)
    );
    assert_eq!(
        stake_pool.preferred_withdraw_validator_vote_address,
        Some(zeta_vote::ID)
    );

    let ValidatorList {
        header: ValidatorListHeader { max_validators, .. },
        validators,
    } = ValidatorList::deserialize(&mut bc.get_account_data(validator_list_pk).await.as_slice())
        .unwrap();
    assert_eq!(MAX_VALIDATORS, max_validators);
    assert_eq!(usize::try_from(MAX_VALIDATORS).unwrap(), validators.len());
    assert!(validators
        .iter()
        .any(|vsi| vsi.vote_account_address == shinobi_vote::ID));
    assert!(validators
        .iter()
        .any(|vsi| vsi.vote_account_address == zeta_vote::ID));
}
