use std::num::NonZeroU32;

use borsh::BorshSerialize;
use sanctum_solana_test_utils::{
    default_rent_exempt_lamports,
    stake::{StakeProgramTest, StakeStateAndLamports},
    token::{tokenkeg::TokenkegProgramTest, MockMintArgs},
    ExtendedProgramTest, IntoAccount, KeyedAccount,
};
use sanctum_spl_stake_pool_lib::{
    FindTransientStakeAccount, FindTransientStakeAccountArgs, FindValidatorStakeAccount,
    FindValidatorStakeAccountArgs, FindWithdrawAuthority, STAKE_POOL_SIZE,
};
use solana_program_test::ProgramTest;
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{
    account::Account,
    pubkey::Pubkey,
    rent::Rent,
    stake::{
        stake_flags::StakeFlags,
        state::{Authorized, Delegation, Lockup, Meta, Stake, StakeStateV2},
    },
};
use spl_stake_pool_interface::{
    AccountType, StakePool, StakeStatus, ValidatorList, ValidatorListHeader, ValidatorStakeInfo,
};

pub struct PoolKeys {
    pub pool: Pubkey,
    pub validator_list: Pubkey,
    pub reserve: Pubkey,
    pub mint: Pubkey,
}

impl PoolKeys {
    pub fn gen() -> Self {
        let [pool, validator_list, reserve, mint] = [0; 4].map(|_| Pubkey::new_unique());
        Self {
            pool,
            validator_list,
            reserve,
            mint,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PoolArgs {
    pub program: Pubkey,
    pub pool: Pubkey,
    pub current_epoch: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ValidatorArgs {
    pub vsa_activation_epoch: u64,
    pub transient_state: TransientStakeAccountState,
}

/// Adds:
/// - mint account (assumed tokenkeg)
/// - validators' validator stake accounts
/// - reserve account
/// - stake pool account
/// - validator list account
pub fn add_all_stake_pool_accounts(
    pt: ProgramTest,
    pool_args: PoolArgs,
    stake_pool: &StakePool,
    validator_list: &ValidatorList,
    validator_args: &[ValidatorArgs],
) -> ProgramTest {
    if validator_args.len() != validator_list.validators.len() {
        panic!("transient_states len does not match validator list len");
    }
    if stake_pool.token_program != spl_token_interface::ID {
        panic!("Only support tokenkeg progrm for now");
    }
    let PoolArgs { program, pool, .. } = pool_args;
    // add mint account
    let (withdraw_auth, _bump) = FindWithdrawAuthority { pool }.run_for_prog(&program);
    let pt = pt.add_tokenkeg_mint_from_args(
        stake_pool.pool_mint,
        MockMintArgs {
            mint_authority: Some(withdraw_auth),
            freeze_authority: None,
            supply: stake_pool.pool_token_supply,
            decimals: 9,
        },
    );
    // add validator stake accounts
    let pt = validator_list
        .validators
        .iter()
        .zip(validator_args)
        .fold(pt, |pt, (vsi, args)| {
            let ValidatorPoolStakeAccounts {
                validator,
                transient,
            } = validator_pool_stake_accounts(pool_args, *args, vsi);
            [validator, transient]
                .into_iter()
                .flatten() // filter_map with identity
                .fold(pt, |pt, ka| pt.add_keyed_account(ka))
        });
    // add reserve account
    let total_validator_stake_lamports = validator_list.validators.iter().fold(0, |sum, vsi| {
        sum + vsi.active_stake_lamports + vsi.transient_stake_lamports
    });
    let reserve_stake = stake_pool.total_lamports - total_validator_stake_lamports;
    // stake_pool.total_lamports does NOT include reserve's rent reserve
    let reserve_balance = reserve_stake + default_rent_exempt_lamports(StakeStateV2::size_of());
    let pt = pt.add_fresh_inactive_stake_account(
        stake_pool.reserve_stake,
        reserve_balance,
        Authorized::auto(&withdraw_auth),
    );
    // add stake pool and validator list
    pt.add_account_chained(
        pool,
        MockPool {
            pool: stake_pool,
            program,
        }
        .into_account(),
    )
    .add_account_chained(
        stake_pool.validator_list,
        MockValidatorList {
            validator_list,
            program,
        }
        .into_account(),
    )
}

#[derive(Clone, Debug, Default)]
pub struct ValidatorPoolStakeAccounts {
    pub validator: Option<KeyedAccount>,
    pub transient: Option<KeyedAccount>,
}

#[allow(unused)] // TODO: remove allow(unused)
#[derive(Clone, Copy, Debug)]
pub enum TransientStakeAccountState {
    Activating,
    Deactivating,
}

pub fn validator_pool_stake_accounts(
    PoolArgs {
        program,
        pool,
        current_epoch,
    }: PoolArgs,
    ValidatorArgs {
        vsa_activation_epoch,
        transient_state,
    }: ValidatorArgs,
    ValidatorStakeInfo {
        active_stake_lamports,
        transient_stake_lamports,
        transient_seed_suffix,
        validator_seed_suffix,
        vote_account_address,
        ..
    }: &ValidatorStakeInfo,
) -> ValidatorPoolStakeAccounts {
    let mut res = ValidatorPoolStakeAccounts::default();
    let rent_exempt_reserve = default_rent_exempt_lamports(StakeStateV2::size_of());
    let authorized = Authorized::auto(&FindWithdrawAuthority { pool }.run_for_prog(&program).0);
    if *active_stake_lamports > 0 {
        let stake_state = StakeStateV2::Stake(
            Meta {
                rent_exempt_reserve,
                authorized,
                lockup: Lockup::default(),
            },
            Stake {
                delegation: Delegation {
                    voter_pubkey: *vote_account_address,
                    stake: active_stake_lamports - rent_exempt_reserve,
                    activation_epoch: vsa_activation_epoch,
                    deactivation_epoch: u64::MAX,
                    ..Default::default()
                },
                credits_observed: 0,
            },
            StakeFlags::empty(),
        );
        res.validator = Some(Keyed {
            pubkey: FindValidatorStakeAccount::new(FindValidatorStakeAccountArgs {
                pool,
                vote: *vote_account_address,
                seed: NonZeroU32::new(*validator_seed_suffix),
            })
            .run_for_prog(&program)
            .0,
            account: StakeStateAndLamports {
                total_lamports: *active_stake_lamports,
                stake_state,
            }
            .into_account(),
        })
    }
    if *transient_stake_lamports > 0 {
        let (activation_epoch, deactivation_epoch) = match transient_state {
            TransientStakeAccountState::Activating => (current_epoch, u64::MAX),
            TransientStakeAccountState::Deactivating => (vsa_activation_epoch, current_epoch),
        };
        let stake_state = StakeStateV2::Stake(
            Meta {
                rent_exempt_reserve,
                authorized,
                lockup: Lockup::default(),
            },
            Stake {
                delegation: Delegation {
                    voter_pubkey: *vote_account_address,
                    stake: transient_stake_lamports - rent_exempt_reserve,
                    activation_epoch,
                    deactivation_epoch,
                    ..Default::default()
                },
                credits_observed: 0,
            },
            StakeFlags::empty(),
        );
        res.validator = Some(Keyed {
            pubkey: FindTransientStakeAccount::new(FindTransientStakeAccountArgs {
                pool,
                vote: *vote_account_address,
                seed: *transient_seed_suffix,
            })
            .run_for_prog(&program)
            .0,
            account: StakeStateAndLamports {
                total_lamports: *transient_stake_lamports,
                stake_state,
            }
            .into_account(),
        })
    }
    res
}

pub struct MockPool<'a> {
    pub program: Pubkey,
    pub pool: &'a StakePool,
}

impl<'a> IntoAccount for MockPool<'a> {
    fn into_account(self) -> Account {
        let mut data = vec![0; STAKE_POOL_SIZE];
        self.pool.serialize(&mut data.as_mut_slice()).unwrap();
        Account {
            lamports: Rent::default().minimum_balance(STAKE_POOL_SIZE),
            data,
            owner: self.program,
            executable: false,
            rent_epoch: u64::MAX,
        }
    }
}

pub struct MockValidatorList<'a> {
    pub program: Pubkey,
    pub validator_list: &'a ValidatorList,
}

impl<'a> IntoAccount for MockValidatorList<'a> {
    fn into_account(self) -> Account {
        let max_validators = self.validator_list.header.max_validators;
        // like to use get_instance_packed_len() here
        // but thats only available on borsh ^1.0
        let dummy_validator_list = ValidatorList {
            header: ValidatorListHeader {
                account_type: AccountType::ValidatorList,
                max_validators,
            },
            validators: vec![
                ValidatorStakeInfo {
                    active_stake_lamports: 0,
                    transient_stake_lamports: 0,
                    last_update_epoch: 0,
                    transient_seed_suffix: 0,
                    unused: 0,
                    validator_seed_suffix: 0,
                    status: StakeStatus::ReadyForRemoval,
                    vote_account_address: Pubkey::default()
                };
                max_validators.try_into().unwrap()
            ],
        };
        let validator_list_size = dummy_validator_list.try_to_vec().unwrap().len();
        let mut data = vec![0; validator_list_size];
        self.validator_list
            .serialize(&mut data.as_mut_slice())
            .unwrap();
        Account {
            lamports: Rent::default().minimum_balance(validator_list_size),
            data,
            owner: self.program,
            executable: false,
            rent_epoch: u64::MAX,
        }
    }
}
