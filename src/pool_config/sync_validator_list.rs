use std::{collections::HashSet, fmt::Display, iter::Flatten, num::NonZeroU32};

use sanctum_spl_stake_pool_lib::{
    lamports_for_new_vsa, FindEphemeralStakeAccount, FindEphemeralStakeAccountArgs,
    FindTransientStakeAccount, FindTransientStakeAccountArgs, FindValidatorStakeAccount,
    FindValidatorStakeAccountArgs, FindWithdrawAuthority,
};
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    rent::Rent,
    signer::Signer,
    stake::{self, state::StakeStateV2},
    system_program, sysvar,
};
use spl_stake_pool_interface::{
    add_validator_to_pool_ix_with_program_id,
    decrease_additional_validator_stake_ix_with_program_id,
    remove_validator_from_pool_ix_with_program_id, set_preferred_validator_ix_with_program_id,
    AddValidatorToPoolIxArgs, AddValidatorToPoolKeys, AdditionalValidatorStakeArgs,
    DecreaseAdditionalValidatorStakeIxArgs, DecreaseAdditionalValidatorStakeKeys,
    PreferredValidatorType, RemoveValidatorFromPoolKeys, SetPreferredValidatorIxArgs,
    SetPreferredValidatorKeys, StakePool, ValidatorStakeInfo,
};

use crate::pool_config::utils::pubkey_opt_display;

/// All generated ixs must be signed by staker only.
/// Adds and removes validators from the list to match `self.validators`
#[derive(Debug)]
pub struct SyncValidatorListConfig<'a> {
    pub program_id: Pubkey,
    pub payer: &'a (dyn Signer + 'static),
    pub staker: &'a (dyn Signer + 'static),
    pub pool: Pubkey,
    pub validator_list: Pubkey,
    pub reserve: Pubkey,
    pub validators: HashSet<Pubkey>,
    pub preferred_deposit_validator: Option<Pubkey>,
    pub preferred_withdraw_validator: Option<Pubkey>,
    pub rent: &'a Rent,
}

#[derive(Clone, Debug)]
pub struct PreferredValidatorChange {
    pub ty: PreferredValidatorType,
    pub old: Option<Pubkey>,
    pub new: Option<Pubkey>,
}

impl Display for PreferredValidatorChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let attr = match self.ty {
            PreferredValidatorType::Deposit => "deposit",
            PreferredValidatorType::Withdraw => "withdraw",
        };
        write!(
            f,
            "Change preferred {attr} validator from {} to {}",
            pubkey_opt_display(&self.old),
            pubkey_opt_display(&self.new)
        )
    }
}

impl<'a> SyncValidatorListConfig<'a> {
    pub fn signers_maybe_dup(&self) -> [&'a dyn Signer; 2] {
        [self.payer, self.staker]
    }

    fn withdraw_auth(&self) -> Pubkey {
        FindWithdrawAuthority { pool: self.pool }
            .run_for_prog(&self.program_id)
            .0
    }
}

// set preferred validators
impl<'a> SyncValidatorListConfig<'a> {
    pub fn preferred_validator_changeset(
        &self,
        stake_pool: &StakePool,
    ) -> impl Iterator<Item = PreferredValidatorChange> + Clone {
        [
            (
                self.preferred_deposit_validator,
                stake_pool.preferred_deposit_validator_vote_address,
                PreferredValidatorType::Deposit,
            ),
            (
                self.preferred_withdraw_validator,
                stake_pool.preferred_withdraw_validator_vote_address,
                PreferredValidatorType::Withdraw,
            ),
        ]
        .into_iter()
        .filter_map(|(new, old, ty)| {
            if new != old {
                Some(PreferredValidatorChange { ty, new, old })
            } else {
                None
            }
        })
    }

    pub fn preferred_validator_ixs(
        &self,
        changes: impl Iterator<Item = PreferredValidatorChange>,
    ) -> std::io::Result<Vec<Instruction>> {
        changes
            .map(|c| self.set_preferred_validator_ix(c))
            .collect()
    }

    fn set_preferred_validator_ix(
        &self,
        PreferredValidatorChange { ty, new, .. }: PreferredValidatorChange,
    ) -> std::io::Result<Instruction> {
        set_preferred_validator_ix_with_program_id(
            self.program_id,
            SetPreferredValidatorKeys {
                stake_pool: self.pool,
                staker: self.staker.pubkey(),
                validator_list: self.validator_list,
            },
            SetPreferredValidatorIxArgs {
                validator_type: ty,
                validator_vote_address: new,
            },
        )
    }
}

// add/remove validators
impl<'a> SyncValidatorListConfig<'a> {
    /// Returns (add, remove)
    pub fn add_remove_changeset<'me>(
        &'me self,
        validator_list: &'me [ValidatorStakeInfo],
    ) -> (
        impl Iterator<Item = &'me Pubkey> + Clone,
        impl Iterator<Item = &'me ValidatorStakeInfo> + Clone,
    ) {
        (
            self.validators.iter().filter(|v| {
                !validator_list
                    .iter()
                    .any(|vsi| vsi.vote_account_address == **v)
            }),
            validator_list
                .iter()
                .filter(|vsi| !self.validators.contains(&vsi.vote_account_address)),
        )
    }

    pub fn add_validators_ixs<'b>(
        &self,
        add: impl Iterator<Item = &'b Pubkey>,
    ) -> std::io::Result<Vec<Instruction>> {
        add.map(|vote| self.add_validator_ix(vote)).collect()
    }

    fn add_validator_ix(&self, vote: &Pubkey) -> std::io::Result<Instruction> {
        add_validator_to_pool_ix_with_program_id(
            self.program_id,
            AddValidatorToPoolKeys {
                stake_pool: self.pool,
                staker: self.staker.pubkey(),
                reserve_stake: self.reserve,
                withdraw_authority: FindWithdrawAuthority { pool: self.pool }
                    .run_for_prog(&self.program_id)
                    .0,
                validator_list: self.validator_list,
                validator_stake_account: FindValidatorStakeAccount {
                    pool: self.pool,
                    vote: *vote,
                    seed: None,
                }
                .run_for_prog(&self.program_id)
                .0,
                vote_account: *vote,
                rent: sysvar::rent::ID,
                clock: sysvar::clock::ID,
                stake_history: sysvar::stake_history::ID,
                stake_config: stake::config::ID,
                system_program: system_program::ID,
                stake_program: stake::program::ID,
            },
            AddValidatorToPoolIxArgs { optional_seed: 0 },
        )
    }

    pub fn remove_validators_ixs<'b>(
        &self,
        remove: impl Iterator<Item = (&'b ValidatorStakeInfo, StakeStateV2)>,
    ) -> std::io::Result<Vec<Instruction>> {
        let mut res = vec![];
        for (vsi, vsa) in remove {
            res.extend(self.remove_validator_ixs(vsi, vsa)?.into_iter())
        }
        Ok(res)
    }

    /// Assumes vsi has been updated for this epoch
    fn remove_validator_ixs(
        &self,
        ValidatorStakeInfo {
            active_stake_lamports,
            transient_seed_suffix,
            validator_seed_suffix,
            vote_account_address,
            ..
        }: &ValidatorStakeInfo,
        vsa: StakeStateV2,
    ) -> std::io::Result<RemoveValidatorIxs> {
        let validator_stake_account =
            FindValidatorStakeAccount::new(FindValidatorStakeAccountArgs {
                pool: self.pool,
                vote: *vote_account_address,
                seed: NonZeroU32::new(*validator_seed_suffix),
            })
            .run_for_prog(&self.program_id)
            .0;
        let transient_stake_account =
            FindTransientStakeAccount::new(FindTransientStakeAccountArgs {
                pool: self.pool,
                vote: *vote_account_address,
                seed: *transient_seed_suffix,
            })
            .run_for_prog(&self.program_id)
            .0;
        let remove_ix = remove_validator_from_pool_ix_with_program_id(
            self.program_id,
            RemoveValidatorFromPoolKeys {
                stake_pool: self.pool,
                staker: self.staker.pubkey(),
                withdraw_authority: self.withdraw_auth(),
                validator_list: self.validator_list,
                validator_stake_account,
                transient_stake_account,
                clock: sysvar::clock::ID,
                stake_program: stake::program::ID,
            },
        )?;
        let lamports_to_decrease =
            active_stake_lamports.saturating_sub(lamports_for_new_vsa(self.rent));
        let is_vsa_active = match vsa {
            StakeStateV2::Stake(_meta, stake, _flags) => {
                stake.delegation.deactivation_epoch == u64::MAX
            }
            _ => false,
        };
        Ok(if lamports_to_decrease > 0 && is_vsa_active {
            let ephemeral_stake_seed = 0;
            let decrease_ix = decrease_additional_validator_stake_ix_with_program_id(
                self.program_id,
                DecreaseAdditionalValidatorStakeKeys {
                    stake_pool: self.pool,
                    staker: self.staker.pubkey(),
                    withdraw_authority: self.withdraw_auth(),
                    validator_list: self.validator_list,
                    reserve_stake: self.reserve,
                    validator_stake_account,
                    ephemeral_stake_account: FindEphemeralStakeAccount::new(
                        FindEphemeralStakeAccountArgs {
                            pool: self.pool,
                            seed: ephemeral_stake_seed,
                        },
                    )
                    .run_for_prog(&self.program_id)
                    .0,
                    transient_stake_account,
                    clock: sysvar::clock::ID,
                    stake_history: sysvar::stake_history::ID,
                    system_program: system_program::ID,
                    stake_program: stake::program::ID,
                },
                DecreaseAdditionalValidatorStakeIxArgs {
                    args: AdditionalValidatorStakeArgs {
                        lamports: lamports_to_decrease,
                        transient_stake_seed: *transient_seed_suffix,
                        ephemeral_stake_seed,
                    },
                },
            )?;
            RemoveValidatorIxs::WithDecreaseStake(decrease_ix, remove_ix)
        } else {
            RemoveValidatorIxs::RemoveDirectly(remove_ix)
        })
    }
}

#[derive(Debug)]
pub enum RemoveValidatorIxs {
    WithDecreaseStake(Instruction, Instruction),
    RemoveDirectly(Instruction),
}

impl IntoIterator for RemoveValidatorIxs {
    type Item = Instruction;

    type IntoIter = Flatten<std::array::IntoIter<Option<Instruction>, 2>>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::WithDecreaseStake(i1, i2) => [Some(i1), Some(i2)].into_iter().flatten(),
            Self::RemoveDirectly(i) => [Some(i), None].into_iter().flatten(),
        }
    }
}

pub fn print_removing_validators_msg<'a>(remove: impl Iterator<Item = &'a ValidatorStakeInfo>) {
    eprint!("Removing validators: ");
    for to_remove in remove {
        eprint!("{}, ", to_remove.vote_account_address);
    }
    eprintln!();
}

pub fn print_adding_validators_msg<'a>(add: impl Iterator<Item = &'a Pubkey>) {
    eprint!("Adding validators: ");
    for to_add in add {
        eprint!("{to_add}, ");
    }
    eprintln!();
}
