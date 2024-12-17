use std::cmp::Ordering;

use sanctum_solana_cli_utils::TokenAmt;
use sanctum_spl_stake_pool_lib::{
    lamports_for_new_vsa, FindEphemeralStakeAccount, FindEphemeralStakeAccountArgs,
    FindTransientStakeAccount, FindTransientStakeAccountArgs, FindValidatorStakeAccount,
    FindWithdrawAuthority,
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
    decrease_additional_validator_stake_ix_with_program_id,
    increase_additional_validator_stake_ix_with_program_id, AdditionalValidatorStakeArgs,
    DecreaseAdditionalValidatorStakeIxArgs, DecreaseAdditionalValidatorStakeKeys,
    IncreaseAdditionalValidatorStakeIxArgs, IncreaseAdditionalValidatorStakeKeys, StakeStatus,
    ValidatorStakeInfo,
};

/// All generated ixs must be signed by staker only.
#[derive(Debug)]
pub struct SyncDelegationConfig<'a> {
    pub program_id: Pubkey,
    pub payer: &'a (dyn Signer + 'static),
    pub staker: &'a (dyn Signer + 'static),
    pub pool: Pubkey,
    pub validator_list: Pubkey,
    pub reserve: Pubkey,

    /// Note: this is accountinfo.lamports and includes rent-exempt lamports
    pub reserve_lamports: u64,

    pub curr_epoch: u64,
    pub rent: Rent,
}

#[derive(Debug, Clone, Copy)]
pub struct ValidatorDelegationChange {
    pub vote: Pubkey,
    pub transient_seed_suffix: u64,
    pub ty: ValidatorDelegationChangeTy,
}

#[derive(Debug, Clone, Copy)]
pub enum ValidatorDelegationChangeTy {
    DecreaseStake(u64),
    IncreaseStake(u64),

    /// Pool tries to increase to the best of abilities, but reserve
    /// doesnt have enough lamports to increase to desired amount
    PartialIncreaseStake {
        increase: u64,
        shortfall: u64,
    },

    InsufficientReserveLamports,
    NoChange,
    TransientWrongState,
    ValidatorBeingRemoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientStakeAccStatus {
    Deactivating,
    Activating,
    None,
}

pub fn next_epoch_stake_and_transient_status(
    vsa: &StakeStateV2,
    tsa: &Option<StakeStateV2>,
    curr_epoch: u64,
) -> (u64, TransientStakeAccStatus) {
    let vsa_stake = vsa.delegation().unwrap().stake;
    match tsa {
        None => (vsa_stake, TransientStakeAccStatus::None),
        Some(ss) => {
            let dlgt = ss.delegation().unwrap();
            let transient_stake = dlgt.stake;
            if dlgt.activation_epoch == curr_epoch {
                (
                    vsa_stake.saturating_add(transient_stake),
                    TransientStakeAccStatus::Activating,
                )
            } else {
                (
                    vsa_stake.saturating_sub(transient_stake),
                    TransientStakeAccStatus::Deactivating,
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DelegationChangeset<D> {
    delegations: D,
    reserve_lamports: u64,
    curr_epoch: u64,
    rent: Rent,
}

/// `(validator_stake_info, validator_stake_account_state, transient_stake_account_state, desired_stake_lamports)`
type ValidatorChangeSrc<'a> = (
    &'a ValidatorStakeInfo,
    &'a StakeStateV2,
    &'a Option<StakeStateV2>,
    u64,
);

impl<'a, D: Iterator<Item = ValidatorChangeSrc<'a>>> DelegationChangeset<D> {
    pub const fn new(delegations: D, reserve_lamports: u64, curr_epoch: u64, rent: Rent) -> Self {
        Self {
            delegations,
            reserve_lamports,
            curr_epoch,
            rent,
        }
    }

    pub fn print_decrease_stakes(self) {
        self.print_summary(
            "Decreasing stake:",
            |ValidatorDelegationChange { ty, vote, .. }| match ty {
                ValidatorDelegationChangeTy::DecreaseStake(dec) => Some(format!(
                    "{} SOL from {vote}",
                    TokenAmt {
                        amt: dec,
                        decimals: 9
                    }
                )),
                _ => None,
            },
        );
    }

    pub fn print_increase_stakes(self) {
        self.print_summary(
            "Increasing stake:",
            |ValidatorDelegationChange { ty, vote, .. }| match ty {
                ValidatorDelegationChangeTy::IncreaseStake(inc) => Some(format!(
                    "{} SOL to {vote}",
                    TokenAmt {
                        amt: inc,
                        decimals: 9
                    }
                )),
                _ => None,
            },
        );
    }

    pub fn print_partial_increase_stakes(self) {
        self.print_summary(
            "Partially increasing stake:",
            |ValidatorDelegationChange { ty, vote, .. }| match ty {
                ValidatorDelegationChangeTy::PartialIncreaseStake {
                    increase,
                    shortfall,
                } => Some(format!(
                    "{} SOL to {vote} ({} SOL shortfall)",
                    TokenAmt {
                        amt: increase,
                        decimals: 9
                    },
                    TokenAmt {
                        amt: shortfall,
                        decimals: 9
                    }
                )),
                _ => None,
            },
        );
    }

    pub fn print_insufficient_reserve_lamports(self) {
        self.print_summary(
            "Insufficient reserve lamports to increase stake to:",
            |ValidatorDelegationChange { ty, vote, .. }| match ty {
                ValidatorDelegationChangeTy::InsufficientReserveLamports => Some(format!("{vote}")),
                _ => None,
            },
        );
    }

    pub fn print_transient_wrong_states(self) {
        self.print_summary(
            "Transient stake account in wrong state to make changes for:",
            |ValidatorDelegationChange { ty, vote, .. }| match ty {
                ValidatorDelegationChangeTy::TransientWrongState => Some(format!("{vote}")),
                _ => None,
            },
        );
    }

    fn print_summary(
        self,
        header: &str,
        filter_map_fn: fn(ValidatorDelegationChange) -> Option<String>,
    ) {
        let mut itr = self.filter_map(filter_map_fn).peekable();
        if itr.peek().is_none() {
            return;
        }
        eprint!("{header} ");
        for msg in itr {
            eprint!("{msg}, ");
        }
        eprintln!();
    }
}

impl<'a, D: Iterator<Item = ValidatorChangeSrc<'a>> + Clone> DelegationChangeset<D> {
    pub fn print_all_changes(&self) {
        self.clone().print_decrease_stakes();
        self.clone().print_increase_stakes();
        self.clone().print_partial_increase_stakes();
        self.clone().print_insufficient_reserve_lamports();
        self.clone().print_transient_wrong_states();
    }
}

impl<'a, D: Iterator<Item = ValidatorChangeSrc<'a>>> Iterator for DelegationChangeset<D> {
    type Item = ValidatorDelegationChange;

    fn next(&mut self) -> Option<Self::Item> {
        let (vsi, vsa, tsa, desired) = self.delegations.next()?;
        if vsi.status != StakeStatus::Active {
            return Some(ValidatorDelegationChange {
                vote: vsi.vote_account_address,
                transient_seed_suffix: vsi.transient_seed_suffix,
                ty: ValidatorDelegationChangeTy::ValidatorBeingRemoved,
            });
        }
        let (next_epoch_stake, tsa_status) =
            next_epoch_stake_and_transient_status(vsa, tsa, self.curr_epoch);
        match next_epoch_stake.cmp(&desired) {
            Ordering::Greater => Some(match tsa_status {
                TransientStakeAccStatus::Deactivating | TransientStakeAccStatus::None => {
                    ValidatorDelegationChange {
                        vote: vsi.vote_account_address,
                        transient_seed_suffix: vsi.transient_seed_suffix,
                        ty: ValidatorDelegationChangeTy::DecreaseStake(next_epoch_stake - desired),
                    }
                }
                TransientStakeAccStatus::Activating => ValidatorDelegationChange {
                    vote: vsi.vote_account_address,
                    transient_seed_suffix: vsi.transient_seed_suffix,
                    ty: ValidatorDelegationChangeTy::TransientWrongState,
                },
            }),
            Ordering::Equal => Some(ValidatorDelegationChange {
                vote: vsi.vote_account_address,
                transient_seed_suffix: vsi.transient_seed_suffix,
                ty: ValidatorDelegationChangeTy::NoChange,
            }),
            Ordering::Less => Some(match tsa_status {
                TransientStakeAccStatus::Activating | TransientStakeAccStatus::None => {
                    let sa_rent_lamports = lamports_for_new_vsa(&self.rent);
                    // https://github.com/solana-labs/solana-program-library/blob/d4b7fc06233b11efecc082cd2f6ee3eadd5daa04/stake-pool/program/src/processor.rs#L1635-L1643
                    let available_stake =
                        self.reserve_lamports.saturating_sub(2 * sa_rent_lamports);
                    let desired_inc = desired - next_epoch_stake;
                    let actual_inc = std::cmp::min(available_stake, desired_inc);
                    self.reserve_lamports -= actual_inc;
                    ValidatorDelegationChange {
                        vote: vsi.vote_account_address,
                        transient_seed_suffix: vsi.transient_seed_suffix,
                        ty: if actual_inc == desired_inc {
                            ValidatorDelegationChangeTy::IncreaseStake(actual_inc)
                        } else if actual_inc == 0 {
                            ValidatorDelegationChangeTy::InsufficientReserveLamports
                        } else {
                            ValidatorDelegationChangeTy::PartialIncreaseStake {
                                increase: actual_inc,
                                shortfall: desired_inc - actual_inc,
                            }
                        },
                    }
                }
                TransientStakeAccStatus::Deactivating => ValidatorDelegationChange {
                    vote: vsi.vote_account_address,
                    transient_seed_suffix: vsi.transient_seed_suffix,
                    ty: ValidatorDelegationChangeTy::TransientWrongState,
                },
            }),
        }
    }
}

impl<'a> SyncDelegationConfig<'a> {
    pub fn signers_maybe_dup(&self) -> [&'a dyn Signer; 2] {
        [self.payer, self.staker]
    }

    fn withdraw_auth(&self) -> Pubkey {
        FindWithdrawAuthority { pool: self.pool }
            .run_for_prog(&self.program_id)
            .0
    }

    pub fn changeset<
        'b,
        D: Iterator<
            Item = (
                &'b ValidatorStakeInfo,
                &'b StakeStateV2,
                &'b Option<StakeStateV2>,
                u64,
            ),
        >,
    >(
        &self,
        delegations: D,
    ) -> DelegationChangeset<D> {
        DelegationChangeset::new(
            delegations,
            self.reserve_lamports,
            self.curr_epoch,
            self.rent,
        )
    }

    pub fn sync_delegation_ixs(
        &self,
        itr: impl Iterator<Item = ValidatorDelegationChange>,
    ) -> impl Iterator<Item = Instruction> {
        let ephemeral_stake_seed = 0;
        let stake_pool = self.pool;
        let staker = self.staker.pubkey();
        let withdraw_authority = self.withdraw_auth();
        let validator_list = self.validator_list;
        let reserve_stake = self.reserve;
        let program_id = self.program_id;
        itr.filter_map(
            move |ValidatorDelegationChange {
                      vote,
                      ty,
                      transient_seed_suffix,
                  }| {
                let (validator_stake_account, _bump) = FindValidatorStakeAccount {
                    pool: stake_pool,
                    vote,
                    seed: None,
                }
                .run_for_prog(&program_id);
                let (ephemeral_stake_account, _bump) =
                    FindEphemeralStakeAccount::new(FindEphemeralStakeAccountArgs {
                        pool: stake_pool,
                        seed: ephemeral_stake_seed,
                    })
                    .run_for_prog(&program_id);
                let (transient_stake_account, _bump) =
                    FindTransientStakeAccount::new(FindTransientStakeAccountArgs {
                        pool: stake_pool,
                        vote,
                        seed: transient_seed_suffix,
                    })
                    .run_for_prog(&program_id);
                match ty {
                    ValidatorDelegationChangeTy::DecreaseStake(dec) => Some(
                        decrease_additional_validator_stake_ix_with_program_id(
                            program_id,
                            DecreaseAdditionalValidatorStakeKeys {
                                stake_pool,
                                staker,
                                withdraw_authority,
                                validator_list,
                                reserve_stake,
                                validator_stake_account,
                                ephemeral_stake_account,
                                transient_stake_account,
                                clock: sysvar::clock::ID,
                                stake_history: sysvar::stake_history::ID,
                                system_program: system_program::ID,
                                stake_program: stake::program::ID,
                            },
                            DecreaseAdditionalValidatorStakeIxArgs {
                                args: AdditionalValidatorStakeArgs {
                                    lamports: dec,
                                    transient_stake_seed: transient_seed_suffix,
                                    ephemeral_stake_seed,
                                },
                            },
                        )
                        .unwrap(),
                    ),
                    ValidatorDelegationChangeTy::IncreaseStake(inc)
                    | ValidatorDelegationChangeTy::PartialIncreaseStake { increase: inc, .. } => {
                        Some(
                            increase_additional_validator_stake_ix_with_program_id(
                                program_id,
                                IncreaseAdditionalValidatorStakeKeys {
                                    stake_pool,
                                    staker,
                                    withdraw_authority,
                                    validator_list,
                                    reserve_stake,
                                    ephemeral_stake_account,
                                    transient_stake_account,
                                    validator_stake_account,
                                    vote_account: vote,
                                    clock: sysvar::clock::ID,
                                    stake_history: sysvar::stake_history::ID,
                                    stake_config: stake::config::ID,
                                    system_program: system_program::ID,
                                    stake_program: stake::program::ID,
                                },
                                IncreaseAdditionalValidatorStakeIxArgs {
                                    args: AdditionalValidatorStakeArgs {
                                        lamports: inc,
                                        transient_stake_seed: transient_seed_suffix,
                                        ephemeral_stake_seed,
                                    },
                                },
                            )
                            .unwrap(),
                        )
                    }
                    ValidatorDelegationChangeTy::InsufficientReserveLamports
                    | ValidatorDelegationChangeTy::NoChange
                    | ValidatorDelegationChangeTy::TransientWrongState
                    | ValidatorDelegationChangeTy::ValidatorBeingRemoved => None,
                }
            },
        )
    }
}
