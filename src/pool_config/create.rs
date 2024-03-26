use sanctum_spl_stake_pool_lib::account_resolvers::{Initialize, InitializeWithDepositAuthArgs};
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{
    account::Account,
    hash::Hash,
    instruction::Instruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signer::Signer,
};
use spl_stake_pool_interface::{
    initialize_ix_with_program_id, set_fee_ix_with_program_id,
    set_funding_authority_ix_with_program_id, Fee, FeeType, FundingType, InitializeIxArgs,
    SetFeeIxArgs, SetFeeKeys, SetFundingAuthorityIxArgs, SetFundingAuthorityKeys,
};

#[derive(Debug)]
pub struct CreateConfig<'a> {
    pub mint: Keyed<Account>,
    pub program_id: Pubkey,
    pub payer: &'a (dyn Signer + 'static),
    pub pool_keypair: &'a (dyn Signer + 'static),
    pub validator_list_keypair: &'a (dyn Signer + 'static),
    pub reserve: &'a (dyn Signer + 'static),
    pub manager: &'a (dyn Signer + 'static),
    pub manager_fee_account: Pubkey,
    pub staker: Pubkey,
    pub stake_deposit_auth: Option<Pubkey>,
    pub sol_deposit_auth: Option<Pubkey>,
    pub sol_withdraw_auth: Option<Pubkey>,
    pub preferred_deposit_validator: Option<Pubkey>,
    pub preferred_withdraw_validator: Option<Pubkey>,
    pub stake_deposit_referral_fee: u8,
    pub sol_deposit_referral_fee: u8,
    pub epoch_fee: Fee,
    pub stake_withdrawal_fee: Fee,
    pub sol_withdrawal_fee: Fee,
    pub stake_deposit_fee: Fee,
    pub sol_deposit_fee: Fee,
    pub max_validators: u32,
}

impl<'a> CreateConfig<'a> {
    pub fn initialize_msg(&self, rbh: Hash) -> std::io::Result<VersionedMessage> {
        let mut ixs = vec![self.initialize_ix()?];
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
        Ok(VersionedMessage::V0(
            Message::try_compile(&self.payer.pubkey(), &ixs, &[], rbh)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?,
        ))
    }

    pub fn initialize_ix(&self) -> std::io::Result<Instruction> {
        let initialize = Initialize {
            pool_token_mint: &self.mint,
            stake_pool: self.pool_keypair.pubkey(),
            manager: self.manager.pubkey(),
            staker: self.staker,
            validator_list: self.validator_list_keypair.pubkey(),
            reserve_stake: self.reserve.pubkey(),
            manager_fee_account: self.manager_fee_account,
        };
        let mut ix = initialize_ix_with_program_id(
            self.program_id,
            initialize.resolve_for_prog(&self.program_id),
            InitializeIxArgs {
                fee: self.epoch_fee.clone(),
                // initialize ix sets both sol and stake fees to the same number.
                // Use stake deposit as source of truth
                withdrawal_fee: self.stake_withdrawal_fee.clone(),
                deposit_fee: self.stake_deposit_fee.clone(),
                referral_fee: self.stake_deposit_referral_fee,
                max_validators: self.max_validators,
            },
        )?;
        // initialize ix sets both sol and stake deposit auth to the same pubkey if set.
        // Use stake deposit as source of truth
        if let Some(deposit_auth) = self.stake_deposit_auth {
            ix.accounts = Vec::from(initialize.resolve_with_deposit_auth(
                InitializeWithDepositAuthArgs {
                    deposit_auth,
                    program_id: self.program_id,
                },
            ));
        }
        Ok(ix)
    }

    pub fn set_sol_deposit_auth_ix(&self) -> std::io::Result<Option<Instruction>> {
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
                stake_pool: self.pool_keypair.pubkey(),
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
                stake_pool: self.pool_keypair.pubkey(),
                manager: self.manager.pubkey(),
                new_funding_authority: sol_withdraw_auth,
            },
            SetFundingAuthorityIxArgs {
                auth: FundingType::SolWithdraw,
            },
        )
        .map(Some)
    }

    pub fn set_sol_referral_ix(&self) -> std::io::Result<Option<Instruction>> {
        if self.sol_deposit_referral_fee == self.stake_deposit_referral_fee {
            return Ok(None);
        }
        set_fee_ix_with_program_id(
            self.program_id,
            SetFeeKeys {
                stake_pool: self.pool_keypair.pubkey(),
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

    pub fn set_sol_deposit_fee_ix(&self) -> std::io::Result<Option<Instruction>> {
        if self.sol_deposit_fee == self.stake_deposit_fee {
            return Ok(None);
        }
        set_fee_ix_with_program_id(
            self.program_id,
            SetFeeKeys {
                stake_pool: self.pool_keypair.pubkey(),
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

    pub fn set_sol_withdraw_fee_ix(&self) -> std::io::Result<Option<Instruction>> {
        if self.sol_withdrawal_fee == self.stake_withdrawal_fee {
            return Ok(None);
        }
        set_fee_ix_with_program_id(
            self.program_id,
            SetFeeKeys {
                stake_pool: self.pool_keypair.pubkey(),
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
}
