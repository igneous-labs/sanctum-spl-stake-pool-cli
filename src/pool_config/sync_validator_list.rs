use std::{collections::HashSet, num::NonZeroU32};

use sanctum_spl_stake_pool_lib::{
    FindEphemeralStakeAccount, FindEphemeralStakeAccountArgs, FindTransientStakeAccount,
    FindTransientStakeAccountArgs, FindValidatorStakeAccount, FindValidatorStakeAccountArgs,
    FindWithdrawAuthority,
};
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signer::Signer, stake, system_program, sysvar,
};
use spl_stake_pool_interface::{
    add_validator_to_pool_ix_with_program_id,
    decrease_additional_validator_stake_ix_with_program_id,
    remove_validator_from_pool_ix_with_program_id, AddValidatorToPoolIxArgs,
    AddValidatorToPoolKeys, AdditionalValidatorStakeArgs, DecreaseAdditionalValidatorStakeIxArgs,
    DecreaseAdditionalValidatorStakeKeys, RemoveValidatorFromPoolKeys, ValidatorStakeInfo,
};

/// All generated ixs must be signed by staker only.
/// Adds and removes validators from the list to match `self.validators`
/// TODO: SyncDelegationConfig for staker to control delegation every epoch
#[derive(Debug)]
pub struct SyncValidatorListConfig<'a> {
    pub program_id: Pubkey,
    pub payer: &'a (dyn Signer + 'static),
    pub staker: &'a (dyn Signer + 'static),
    pub pool: Pubkey,
    pub validator_list: Pubkey,
    pub reserve: Pubkey,
    pub validators: HashSet<Pubkey>,
}

impl<'a> SyncValidatorListConfig<'a> {
    pub fn signers_maybe_dup(&self) -> [&'a dyn Signer; 2] {
        [self.payer, self.staker]
    }

    /// Returns (add, remove)
    pub fn changeset<'me>(
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

    #[allow(unused)] // TODO: remove
    pub fn remove_validators_ixs<'b>(
        &self,
        remove: impl Iterator<Item = &'b ValidatorStakeInfo>,
    ) -> std::io::Result<Vec<Instruction>> {
        let mut res = vec![];
        for rem_args in remove {
            res.extend(self.remove_validator_ixs(rem_args)?.into_iter())
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
        Ok(if *active_stake_lamports > 0 {
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
                        lamports: *active_stake_lamports,
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

    fn withdraw_auth(&self) -> Pubkey {
        FindWithdrawAuthority { pool: self.pool }
            .run_for_prog(&self.program_id)
            .0
    }
}

#[derive(Debug)]
pub enum RemoveValidatorIxs {
    WithDecreaseStake(Instruction, Instruction),
    RemoveDirectly(Instruction),
}

impl RemoveValidatorIxs {
    pub fn into_iter(self) -> impl Iterator<Item = Instruction> {
        // hax to make both iterators the same type
        let fm = |ix_opt| ix_opt;
        match self {
            Self::WithDecreaseStake(i1, i2) => [Some(i1), Some(i2)].into_iter().filter_map(fm),
            Self::RemoveDirectly(i) => [Some(i), None].into_iter().filter_map(fm),
        }
    }
}
