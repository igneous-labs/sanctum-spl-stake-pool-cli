use sanctum_spl_stake_pool_lib::{
    account_resolvers::{Initialize, InitializeWithDepositAuthArgs},
    FindWithdrawAuthority,
};
use solana_readonly_account::{keyed::Keyed, ReadonlyAccountData, ReadonlyAccountOwner};
use solana_sdk::{
    borsh1::get_instance_packed_len,
    instruction::Instruction,
    pubkey::Pubkey,
    rent::Rent,
    signer::Signer,
    stake::{
        self,
        state::{Authorized, Lockup},
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

// TODO: stake pool program may change parameters
const MIN_RESERVE_BALANCE: u64 = 0;

// TODO: stake pool program may change parameters
const STAKE_POOL_SIZE: usize = 611;

// TODO: stake program may change parameters
const STAKE_STATE_LEN: u64 = 200;

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
    pub rent: Rent,
}

impl<'a, T: ReadonlyAccountOwner + ReadonlyAccountData> CreateConfig<'a, T> {
    // split from initialize_tx due to tx size limits
    pub fn create_reserve_tx_ixs(&self) -> std::io::Result<[Instruction; 2]> {
        let create_reserve_ix = system_instruction::create_account(
            &self.payer.pubkey(),
            &self.reserve.pubkey(),
            MIN_RESERVE_BALANCE
                + self
                    .rent
                    .minimum_balance(STAKE_STATE_LEN.try_into().unwrap()),
            STAKE_STATE_LEN,
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
        let validator_list_size = get_instance_packed_len(&dummy_validator_list)?;
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

    /*
    pub fn initialize_tx_ixs(&self) -> std::io::Result<Vec<Instruction>> {
        let mut ixs = vec![];
        if let Some(ix) = self.create_manager_fee_ata_ix()? {
            ixs.push(ix);
        }
        ixs.extend(self.initialize_ixs()?);
        for ix_res in [
            self.set_sol_deposit_auth_ix(),
            self.set_sol_deposit_fee_ix(),
            self.set_sol_referral_ix(),
            self.set_sol_withdraw_auth_ix(),
            self.set_sol_withdraw_fee_ix(),
        ] {
            if let Some(ix) = ix_res? {
                ixs.push(ix);
            }
        }
        Ok(ixs)
    }

    fn create_manager_fee_ata_ix(&self) -> std::io::Result<Option<Instruction>> {
        if !self.should_create_manager_fee_ata {
            return Ok(None);
        }
        let (
            CreateKeys {
                funding_account,
                associated_token_account,
                wallet,
                mint,
                system_program,
                token_program,
            },
            _bump,
        ) = CreateFreeArgs {
            funding_account: self.payer.pubkey(),
            wallet: self.manager.pubkey(),
            mint: &self.mint,
        }
        .resolve();
        if associated_token_account != self.manager_fee_account {
            return Err(std::io::Error::other(
                "Trying to created non associated manager_fee_account",
            ));
        }
        create_idempotent_ix(CreateIdempotentKeys {
            funding_account,
            associated_token_account,
            wallet,
            mint,
            system_program,
            token_program,
        })
        .map(Some)
    }

    fn set_sol_deposit_auth_ix(&self) -> std::io::Result<Option<Instruction>> {
        let (sol_deposit_auth, is_removing) = match (self.stake_deposit_auth, self.sol_deposit_auth)
        {
            (None, None) => return Ok(None),
            (Some(stake), Some(sol)) => {
                if stake == sol {
                    return Ok(None);
                } else {
                    (sol, false)
                }
            }
            (Some(stake), None) => (stake /* dont care */, true),
            (None, Some(sol)) => (sol, false),
        };
        let mut ix = set_funding_authority_ix_with_program_id(
            self.program_id,
            SetFundingAuthorityKeys {
                stake_pool: self.pool.pubkey(),
                manager: self.manager.pubkey(),
                new_funding_authority: sol_deposit_auth,
            },
            SetFundingAuthorityIxArgs {
                auth: FundingType::SolDeposit,
            },
        )?;
        if is_removing {
            ix.accounts.pop();
        }
        Ok(Some(ix))
    }

    pub fn set_sol_withdraw_auth_ix(&self) -> std::io::Result<Option<Instruction>> {
        let sol_withdraw_auth = match self.sol_withdraw_auth {
            Some(s) => s,
            None => return Ok(None),
        };
        set_funding_authority_ix_with_program_id(
            self.program_id,
            SetFundingAuthorityKeys {
                stake_pool: self.pool.pubkey(),
                manager: self.manager.pubkey(),
                new_funding_authority: sol_withdraw_auth,
            },
            SetFundingAuthorityIxArgs {
                auth: FundingType::SolWithdraw,
            },
        )
        .map(Some)
    }

    fn set_sol_referral_ix(&self) -> std::io::Result<Option<Instruction>> {
        if self.sol_deposit_referral_fee == self.stake_deposit_referral_fee {
            return Ok(None);
        }
        set_fee_ix_with_program_id(
            self.program_id,
            SetFeeKeys {
                stake_pool: self.pool.pubkey(),
                manager: self.manager.pubkey(),
            },
            SetFeeIxArgs {
                fee: FeeType::SolReferral {
                    fee: self.sol_deposit_referral_fee,
                },
            },
        )
        .map(Some)
    }

    fn set_sol_deposit_fee_ix(&self) -> std::io::Result<Option<Instruction>> {
        if self.sol_deposit_fee == self.stake_deposit_fee {
            return Ok(None);
        }
        set_fee_ix_with_program_id(
            self.program_id,
            SetFeeKeys {
                stake_pool: self.pool.pubkey(),
                manager: self.manager.pubkey(),
            },
            SetFeeIxArgs {
                fee: FeeType::SolDeposit {
                    fee: self.sol_deposit_fee.clone(),
                },
            },
        )
        .map(Some)
    }

    fn set_sol_withdraw_fee_ix(&self) -> std::io::Result<Option<Instruction>> {
        if self.sol_withdrawal_fee == self.stake_withdrawal_fee {
            return Ok(None);
        }
        set_fee_ix_with_program_id(
            self.program_id,
            SetFeeKeys {
                stake_pool: self.pool.pubkey(),
                manager: self.manager.pubkey(),
            },
            SetFeeIxArgs {
                fee: FeeType::SolWithdrawal {
                    fee: self.sol_withdrawal_fee.clone(),
                },
            },
        )
        .map(Some)
    }
     */
}

#[cfg(test)]
mod tests {
    use sanctum_associated_token_lib::FindAtaAddressArgs;
    use sanctum_solana_test_utils::{
        test_fixtures_dir,
        token::{tokenkeg::mock_tokenkeg_mint, MockMintArgs},
        IntoAccount, KeyedUiAccount,
    };
    use solana_readonly_account::ReadonlyAccountData;
    use solana_sdk::{
        address_lookup_table::{state::AddressLookupTable, AddressLookupTableAccount},
        compute_budget::ComputeBudgetInstruction,
        hash::Hash,
        message::{v0::Message, VersionedMessage},
        pubkey::Pubkey,
        signature::{Keypair, Signature},
        signer::Signer,
        transaction::VersionedTransaction,
    };

    use crate::test_utils::TX_SIZE_LIMIT;

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
            token_program: spl_token::ID,
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
            rent: Rent::default(),
        };
        let mut ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(0),
            ComputeBudgetInstruction::set_compute_unit_price(0),
        ];
        ixs.extend(config.initialize_tx_ixs().unwrap());

        // compute budget instructions make it go to 1251 without use of srlut
        let srlut =
            KeyedUiAccount::from_file(test_fixtures_dir().join("srlut.json")).to_keyed_account();
        let srlut = AddressLookupTableAccount {
            key: srlut.pubkey,
            addresses: AddressLookupTable::deserialize(&srlut.data())
                .unwrap()
                .addresses
                .into(),
        };

        let tx = VersionedTransaction {
            signatures: vec![Signature::default(); 4],
            message: VersionedMessage::V0(
                Message::try_compile(&payer.pubkey(), &ixs, &[srlut], Hash::default()).unwrap(),
            ),
        };

        let tx_len = bincode::serialize(&tx).unwrap().len();
        // println!("{tx_len}");
        assert!(tx_len < TX_SIZE_LIMIT);
    }
}