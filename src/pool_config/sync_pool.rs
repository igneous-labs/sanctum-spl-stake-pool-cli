use std::fmt::Display;

use sanctum_spl_stake_pool_lib::FindDepositAuthority;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signer::Signer};
use spl_stake_pool_interface::{
    set_fee_ix_with_program_id, set_funding_authority_ix_with_program_id,
    set_manager_ix_with_program_id, set_staker_ix_with_program_id, Fee, FeeType, FundingType,
    FutureEpochFee, SetFeeIxArgs, SetFeeKeys, SetFundingAuthorityIxArgs, SetFundingAuthorityKeys,
    SetManagerKeys, SetStakerKeys, StakePool,
};

/// All generated ixs must be signed by manager only
#[derive(Debug)]
pub struct SyncPoolConfig<'a> {
    pub program_id: Pubkey,
    pub pool: Pubkey,
    pub payer: &'a (dyn Signer + 'static),
    pub manager: &'a (dyn Signer + 'static),
    pub new_manager: &'a (dyn Signer + 'static),
    pub staker: Pubkey,
    pub manager_fee_account: Pubkey,
    pub sol_deposit_auth: Option<Pubkey>,
    pub stake_deposit_auth: Option<Pubkey>,
    pub sol_withdraw_auth: Option<Pubkey>,
    pub sol_deposit_referral_fee: u8,
    pub stake_deposit_referral_fee: u8,
    pub epoch_fee: Fee,
    pub stake_withdrawal_fee: Fee,
    pub sol_withdrawal_fee: Fee,
    pub stake_deposit_fee: Fee,
    pub sol_deposit_fee: Fee,
}

#[derive(Debug, Clone)]
pub enum SyncPoolChange {
    Fee {
        old: FeeType,
        new: FeeType,
    },
    ManagerFeeAccount {
        old: Pubkey,
        new: Pubkey,
    },
    Staker {
        old: Pubkey,
        new: Pubkey,
    },
    Manager {
        old: Pubkey,
        new: Pubkey,
    },
    FundingAuth {
        ty: FundingType,
        old: Option<Pubkey>,
        new: Option<Pubkey>,
    },
}

impl Display for SyncPoolChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Change {} from {} to {}",
            self.attr_name(),
            self.old_val_display(),
            self.new_val_display()
        )
    }
}

impl SyncPoolChange {
    fn attr_name(&self) -> &'static str {
        match self {
            Self::Fee { old, .. } => match old {
                FeeType::Epoch { .. } => "epoch fee",
                FeeType::SolDeposit { .. } => "SOL deposit fee",
                FeeType::SolReferral { .. } => "SOL referral fee",
                FeeType::SolWithdrawal { .. } => "SOL withdrawal fee",
                FeeType::StakeDeposit { .. } => "stake deposit fee",
                FeeType::StakeReferral { .. } => "stake referral fee",
                FeeType::StakeWithdrawal { .. } => "stake withdrawal fee",
            },
            Self::ManagerFeeAccount { .. } => "manager fee account",
            Self::Staker { .. } => "staker",
            Self::Manager { .. } => "manager",
            Self::FundingAuth { ty, .. } => match ty {
                FundingType::SolDeposit => "SOL deposit authority",
                FundingType::SolWithdraw => "SOL withdraw authority",
                FundingType::StakeDeposit => "stake deposit authority",
            },
        }
    }

    fn old_val_display(&self) -> String {
        match self {
            Self::Fee { old, .. } => match old {
                FeeType::SolReferral { fee } | FeeType::StakeReferral { fee } => format!("{fee}%"),
                FeeType::Epoch { fee }
                | FeeType::SolDeposit { fee }
                | FeeType::SolWithdrawal { fee }
                | FeeType::StakeDeposit { fee }
                | FeeType::StakeWithdrawal { fee } => {
                    format!("{}/{}", fee.numerator, fee.denominator)
                }
            },
            Self::ManagerFeeAccount { old, .. } => old.to_string(),
            Self::Staker { old, .. } => old.to_string(),
            Self::Manager { old, .. } => old.to_string(),
            Self::FundingAuth { old, .. } => {
                old.map_or_else(|| "None".to_owned(), |pk| pk.to_string())
            }
        }
    }

    fn new_val_display(&self) -> String {
        match self {
            Self::Fee { new, .. } => match new {
                FeeType::SolReferral { fee } | FeeType::StakeReferral { fee } => format!("{fee}%"),
                FeeType::Epoch { fee }
                | FeeType::SolDeposit { fee }
                | FeeType::SolWithdrawal { fee }
                | FeeType::StakeDeposit { fee }
                | FeeType::StakeWithdrawal { fee } => {
                    format!("{}/{}", fee.numerator, fee.denominator)
                }
            },
            Self::ManagerFeeAccount { new, .. } => new.to_string(),
            Self::Staker { new, .. } => new.to_string(),
            Self::Manager { new, .. } => new.to_string(),
            Self::FundingAuth { new, .. } => {
                new.map_or_else(|| "None".to_owned(), |pk| pk.to_string())
            }
        }
    }
}

impl<'a> SyncPoolConfig<'a> {
    pub fn signers_maybe_dup(&self) -> [&'a dyn Signer; 3] {
        [self.payer, self.manager, self.new_manager]
    }

    pub fn changeset(
        &self,
        StakePool {
            manager,
            staker,
            stake_deposit_authority,
            manager_fee_account,
            epoch_fee,
            next_epoch_fee,
            preferred_deposit_validator_vote_address,
            preferred_withdraw_validator_vote_address,
            stake_deposit_fee,
            stake_withdrawal_fee,
            next_stake_withdrawal_fee,
            stake_referral_fee,
            sol_deposit_authority,
            sol_deposit_fee,
            sol_referral_fee,
            sol_withdraw_authority,
            sol_withdrawal_fee,
            next_sol_withdrawal_fee,
            ..
        }: &StakePool,
    ) -> Vec<SyncPoolChange> {
        let mut res = vec![];
        let (default_deposit_auth, _bump) =
            FindDepositAuthority { pool: self.pool }.run_for_prog(&self.program_id);
        let old_stake_deposit_authority =
            filter_default_stake_deposit_auth(*stake_deposit_authority, &default_deposit_auth);
        let new_stake_deposit_authority = self.stake_deposit_auth.map_or_else(
            || None,
            |pk| filter_default_stake_deposit_auth(pk, &default_deposit_auth),
        );
        for (old_funding_auth, new_funding_auth, ty) in [
            (
                &old_stake_deposit_authority,
                new_stake_deposit_authority,
                FundingType::StakeDeposit,
            ),
            (
                sol_deposit_authority,
                self.sol_deposit_auth,
                FundingType::SolDeposit,
            ),
            (
                sol_withdraw_authority,
                self.sol_withdraw_auth,
                FundingType::SolWithdraw,
            ),
        ] {
            if *old_funding_auth != new_funding_auth {
                res.push(SyncPoolChange::FundingAuth {
                    ty,
                    old: *old_funding_auth,
                    new: new_funding_auth,
                })
            }
        }
        for (old_fee, new_fee, next_fee) in [
            (
                FeeType::Epoch {
                    fee: epoch_fee.clone(),
                },
                FeeType::Epoch {
                    fee: self.epoch_fee.clone(),
                },
                next_epoch_fee,
            ),
            (
                FeeType::SolWithdrawal {
                    fee: sol_withdrawal_fee.clone(),
                },
                FeeType::SolWithdrawal {
                    fee: self.sol_withdrawal_fee.clone(),
                },
                next_sol_withdrawal_fee,
            ),
            (
                FeeType::StakeWithdrawal {
                    fee: stake_withdrawal_fee.clone(),
                },
                FeeType::StakeWithdrawal {
                    fee: self.stake_withdrawal_fee.clone(),
                },
                next_stake_withdrawal_fee,
            ),
        ] {
            if old_fee != new_fee {
                let new_fee_inner = match &new_fee {
                    FeeType::Epoch { fee }
                    | FeeType::SolWithdrawal { fee }
                    | FeeType::StakeWithdrawal { fee } => fee,
                    _ => unreachable!(),
                };
                let should_change = match next_fee {
                    FutureEpochFee::None => true,
                    FutureEpochFee::One { fee } | FutureEpochFee::Two { fee } => {
                        fee != new_fee_inner
                    }
                };
                if should_change {
                    res.push(SyncPoolChange::Fee {
                        old: old_fee,
                        new: new_fee,
                    });
                }
            }
        }
        for (old_fee, new_fee) in [
            (
                FeeType::SolDeposit {
                    fee: sol_deposit_fee.clone(),
                },
                FeeType::SolDeposit {
                    fee: self.sol_deposit_fee.clone(),
                },
            ),
            (
                FeeType::SolReferral {
                    fee: *sol_referral_fee,
                },
                FeeType::SolReferral {
                    fee: self.sol_deposit_referral_fee,
                },
            ),
            (
                FeeType::StakeDeposit {
                    fee: stake_deposit_fee.clone(),
                },
                FeeType::StakeDeposit {
                    fee: self.stake_deposit_fee.clone(),
                },
            ),
            (
                FeeType::StakeReferral {
                    fee: *stake_referral_fee,
                },
                FeeType::StakeReferral {
                    fee: self.stake_deposit_referral_fee,
                },
            ),
        ] {
            if old_fee != new_fee {
                res.push(SyncPoolChange::Fee {
                    old: old_fee,
                    new: new_fee,
                })
            }
        }
        if *staker != self.staker {
            res.push(SyncPoolChange::Staker {
                old: *staker,
                new: self.staker,
            });
        }
        if *manager_fee_account != self.manager_fee_account {
            res.push(SyncPoolChange::ManagerFeeAccount {
                old: *manager_fee_account,
                new: self.manager_fee_account,
            })
        }
        // do manager last so previous changes can be applied first
        let new_manager = self.manager.pubkey();
        if *manager != new_manager {
            res.push(SyncPoolChange::Manager {
                old: *manager,
                new: new_manager,
            })
        }
        res
    }

    fn change_ix(&self, change: &SyncPoolChange) -> std::io::Result<Instruction> {
        match change {
            SyncPoolChange::Fee { new, .. } => self.set_fee_ix(new.clone()),
            SyncPoolChange::ManagerFeeAccount { .. } => self.set_manager_fee_ix(),
            SyncPoolChange::Staker { .. } => self.set_staker_ix(),
            SyncPoolChange::Manager { .. } => self.set_manager_ix(),
            SyncPoolChange::FundingAuth { ty, .. } => self.set_funding_auth_ix(ty.clone()),
        }
    }

    // kinda weird that this uses the arg data instead of self data
    // but set_funding_auth_ix does the opposite. Oh well
    fn set_fee_ix(&self, fee: FeeType) -> std::io::Result<Instruction> {
        set_fee_ix_with_program_id(
            self.program_id,
            SetFeeKeys {
                stake_pool: self.pool,
                manager: self.manager.pubkey(),
            },
            SetFeeIxArgs { fee },
        )
    }

    fn set_funding_auth_ix(&self, auth: FundingType) -> std::io::Result<Instruction> {
        let new_stake_deposit_auth = self.stake_deposit_auth.map_or_else(
            || None,
            |pk| {
                filter_default_stake_deposit_auth(
                    pk,
                    &FindDepositAuthority { pool: self.pool }
                        .run_for_prog(&self.program_id)
                        .0,
                )
            },
        );
        let new_funding_authority = match auth {
            FundingType::StakeDeposit => &new_stake_deposit_auth,
            FundingType::SolDeposit => &self.sol_deposit_auth,
            FundingType::SolWithdraw => &self.sol_withdraw_auth,
        };
        let mut ix = set_funding_authority_ix_with_program_id(
            self.program_id,
            SetFundingAuthorityKeys {
                stake_pool: self.pool,
                manager: self.manager.pubkey(),
                new_funding_authority: new_funding_authority.unwrap_or_default(),
            },
            SetFundingAuthorityIxArgs { auth },
        )?;
        if new_funding_authority.is_none() {
            ix.accounts.pop();
        }
        Ok(ix)
    }

    fn set_manager_ix(&self) -> std::io::Result<Instruction> {
        set_manager_ix_with_program_id(
            self.program_id,
            SetManagerKeys {
                stake_pool: self.pool,
                manager: self.manager.pubkey(),
                new_manager: self.new_manager.pubkey(),
                new_manager_fee_account: self.manager_fee_account,
            },
        )
    }

    fn set_manager_fee_ix(&self) -> std::io::Result<Instruction> {
        set_manager_ix_with_program_id(
            self.program_id,
            SetManagerKeys {
                stake_pool: self.pool,
                manager: self.manager.pubkey(),
                new_manager: self.manager.pubkey(),
                new_manager_fee_account: self.manager_fee_account,
            },
        )
    }

    fn set_staker_ix(&self) -> std::io::Result<Instruction> {
        set_staker_ix_with_program_id(
            self.program_id,
            SetStakerKeys {
                stake_pool: self.pool,
                signer: self.manager.pubkey(),
                new_staker: self.staker,
            },
        )
    }
}

fn filter_default_stake_deposit_auth(
    stake_deposit_auth: Pubkey,
    default_stake_deposit_auth: &Pubkey,
) -> Option<Pubkey> {
    if stake_deposit_auth == *default_stake_deposit_auth {
        None
    } else {
        Some(stake_deposit_auth)
    }
}
