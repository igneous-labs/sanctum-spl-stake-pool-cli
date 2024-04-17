use borsh::BorshSerialize;
use sanctum_spl_stake_pool_lib::{
    account_resolvers::{Initialize, InitializeWithDepositAuthArgs},
    lamports_for_new_vsa, min_reserve_lamports, FindWithdrawAuthority, STAKE_POOL_SIZE,
};
use solana_readonly_account::{keyed::Keyed, ReadonlyAccountData, ReadonlyAccountOwner};
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    rent::Rent,
    signer::Signer,
    stake::{
        self,
        state::{Authorized, Lockup, StakeStateV2},
    },
    system_instruction,
};
use spl_stake_pool_interface::{
    initialize_ix_with_program_id, AccountType, Fee, InitializeIxArgs, StakeStatus, ValidatorList,
    ValidatorListHeader, ValidatorStakeInfo,
};
use spl_token_interface::{
    set_authority_ix_with_program_id, AuthorityType, SetAuthorityIxArgs, SetAuthorityKeys,
};

#[derive(Debug)]
pub struct CreateConfig<'a, T> {
    pub mint: Keyed<T>,
    pub program_id: Pubkey,
    pub payer: &'a (dyn Signer + 'static),
    pub pool: &'a (dyn Signer + 'static),
    pub validator_list: &'a (dyn Signer + 'static),
    pub reserve: &'a (dyn Signer + 'static),
    pub manager: &'a (dyn Signer + 'static),
    pub manager_fee_account: Pubkey,
    pub staker: Pubkey,
    pub deposit_auth: Option<Pubkey>,
    pub deposit_referral_fee: u8,
    pub epoch_fee: Fee,
    pub withdrawal_fee: Fee,
    pub deposit_fee: Fee,
    pub max_validators: u32,
    // Used to calculate how much to fund the reserve by so that the first VSAs can be
    // added immediately
    pub starting_validators: usize,
    pub rent: &'a Rent,
}

impl<'a, T: ReadonlyAccountOwner + ReadonlyAccountData> CreateConfig<'a, T> {
    // split from initialize_tx due to tx size limits
    pub fn create_reserve_tx_ixs(&self) -> std::io::Result<[Instruction; 2]> {
        let create_reserve_ix = system_instruction::create_account(
            &self.payer.pubkey(),
            &self.reserve.pubkey(),
            min_reserve_lamports(self.rent).saturating_add(
                u64::try_from(self.starting_validators)
                    .unwrap()
                    .saturating_mul(lamports_for_new_vsa(self.rent)),
            ),
            std::mem::size_of::<StakeStateV2>().try_into().unwrap(),
            &stake::program::ID,
        );
        let (pool_withdraw_auth, _bump) = FindWithdrawAuthority {
            pool: self.pool.pubkey(),
        }
        .run_for_prog(&self.program_id);
        let init_reserve_ix = stake::instruction::initialize(
            &self.reserve.pubkey(),
            &Authorized::auto(&pool_withdraw_auth),
            &Lockup::default(),
        );
        Ok([create_reserve_ix, init_reserve_ix])
    }

    pub fn create_reserve_tx_signers_maybe_dup(&self) -> [&'a dyn Signer; 2] {
        [self.payer, self.reserve]
    }

    pub fn initialize_tx_signers_maybe_dup(&self) -> [&'a dyn Signer; 4] {
        [self.payer, self.pool, self.validator_list, self.manager]
    }

    // Worst case transaction size is 1251 with 2x computebudget instructions (over limit)
    pub fn initialize_tx_ixs(&self) -> std::io::Result<[Instruction; 4]> {
        let (pool_withdraw_auth, _bump) = FindWithdrawAuthority {
            pool: self.pool.pubkey(),
        }
        .run_for_prog(&self.program_id);
        let transfer_mint_auth_ix = set_authority_ix_with_program_id(
            *self.mint.owner(),
            SetAuthorityKeys {
                account: self.mint.pubkey,
                authority: self.manager.pubkey(),
            },
            SetAuthorityIxArgs {
                authority_type: AuthorityType::MintTokens,
                new_authority: Some(pool_withdraw_auth),
            },
        )?;
        // like to use get_instance_packed_len() here
        // but thats only available on borsh ^1.0
        let dummy_validator_list = ValidatorList {
            header: ValidatorListHeader {
                account_type: AccountType::ValidatorList,
                max_validators: self.max_validators,
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
                self.max_validators.try_into().unwrap()
            ],
        };
        let validator_list_size = dummy_validator_list.try_to_vec()?.len();
        let create_validator_list_ix = system_instruction::create_account(
            &self.payer.pubkey(),
            &self.validator_list.pubkey(),
            self.rent.minimum_balance(validator_list_size),
            validator_list_size.try_into().unwrap(),
            &self.program_id,
        );
        let create_pool_ix = system_instruction::create_account(
            &self.payer.pubkey(),
            &self.pool.pubkey(),
            self.rent.minimum_balance(STAKE_POOL_SIZE),
            STAKE_POOL_SIZE.try_into().unwrap(),
            &self.program_id,
        );
        let initialize = Initialize {
            pool_token_mint: &self.mint,
            stake_pool: self.pool.pubkey(),
            manager: self.manager.pubkey(),
            staker: self.staker,
            validator_list: self.validator_list.pubkey(),
            reserve_stake: self.reserve.pubkey(),
            manager_fee_account: self.manager_fee_account,
        };
        let mut init_ix = initialize_ix_with_program_id(
            self.program_id,
            initialize.resolve_for_prog(&self.program_id),
            InitializeIxArgs {
                fee: self.epoch_fee.clone(),
                // initialize ix sets both sol and stake fees to the same number.
                // Use stake deposit as source of truth
                withdrawal_fee: self.withdrawal_fee.clone(),
                deposit_fee: self.deposit_fee.clone(),
                referral_fee: self.deposit_referral_fee,
                max_validators: self.max_validators,
            },
        )?;
        // initialize ix sets both sol and stake deposit auth to the same pubkey if set.
        // Use stake deposit as source of truth
        if let Some(deposit_auth) = self.deposit_auth {
            init_ix.accounts = Vec::from(initialize.resolve_with_deposit_auth(
                InitializeWithDepositAuthArgs {
                    deposit_auth,
                    program_id: self.program_id,
                },
            ));
        }
        Ok([
            transfer_mint_auth_ix,
            create_validator_list_ix,
            create_pool_ix,
            init_ix,
        ])
    }
}

#[cfg(test)]
mod tests {
    use sanctum_associated_token_lib::FindAtaAddressArgs;
    use sanctum_solana_test_utils::{
        assert_tx_with_cb_ixs_within_size_limits,
        token::{tokenkeg::mock_tokenkeg_mint, MockMintArgs},
        IntoAccount,
    };
    use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};

    use super::*;

    #[test]
    fn create_ixs_tx_size_limit() {
        const N_SIGNERS: usize = 5;

        let keys: Vec<Box<dyn Signer>> = (0..N_SIGNERS)
            .map(|_| Box::new(Keypair::new()).into())
            .collect();
        let [payer, pool, validator_list, reserve, manager]: &[Box<dyn Signer>; N_SIGNERS] =
            keys.as_slice().try_into().unwrap();
        let mint_account = mock_tokenkeg_mint(MockMintArgs {
            mint_authority: Some(Pubkey::new_unique()),
            freeze_authority: None,
            supply: 0,
            decimals: 9,
        })
        .into_account();
        let mint = Pubkey::new_unique();
        let (manager_fee_account, _bump) = FindAtaAddressArgs {
            wallet: manager.pubkey(),
            mint,
            token_program: spl_token_interface::ID,
        }
        .find_ata_address();
        let config = CreateConfig {
            mint: Keyed {
                pubkey: mint,
                account: mint_account,
            },
            program_id: spl_stake_pool_interface::ID,
            payer: payer.as_ref(),
            pool: pool.as_ref(),
            validator_list: validator_list.as_ref(),
            reserve: reserve.as_ref(),
            manager: manager.as_ref(),
            manager_fee_account,
            staker: Pubkey::new_unique(),
            deposit_auth: Some(Pubkey::new_unique()),
            deposit_referral_fee: 1,
            epoch_fee: Fee {
                denominator: 4,
                numerator: 3,
            },
            withdrawal_fee: Fee {
                denominator: 6,
                numerator: 5,
            },
            deposit_fee: Fee {
                denominator: 10,
                numerator: 9,
            },
            max_validators: 13,
            starting_validators: 1,
            rent: &Rent::default(),
        };

        assert_tx_with_cb_ixs_within_size_limits(
            &payer.pubkey(),
            config.initialize_tx_ixs().unwrap().into_iter(),
            &[],
        );
    }
}
