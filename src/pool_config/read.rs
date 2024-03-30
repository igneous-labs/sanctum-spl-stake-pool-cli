//! For updating Config with pool data read from onchain

use sanctum_spl_stake_pool_lib::{FindDepositAuthority, FindWithdrawAuthority};
use solana_sdk::pubkey::Pubkey;
use spl_stake_pool_interface::{StakePool, ValidatorList, ValidatorListHeader};

use super::{ConfigFileRaw, ValidatorConfigRaw};

impl ConfigFileRaw {
    pub fn set_pool_pk(&mut self, pool_pk: Pubkey) {
        self.pool = Some(pool_pk.to_string());
    }

    pub fn set_pool(
        &mut self,
        program_id: &Pubkey,
        pool: Pubkey,
        StakePool {
            manager,
            staker,
            stake_deposit_authority,
            validator_list,
            reserve_stake,
            pool_mint,
            manager_fee_account,
            total_lamports,
            pool_token_supply,
            last_update_epoch,
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
            last_epoch_pool_token_supply,
            last_epoch_total_lamports,
            token_program_id,
            ..
        }: &StakePool,
    ) {
        self.mint = Some(pool_mint.to_string());
        self.token_program = Some(token_program_id.to_string());
        self.validator_list = Some(validator_list.to_string());
        self.reserve = Some(reserve_stake.to_string());
        self.manager = Some(manager.to_string());
        self.manager_fee_account = Some(manager_fee_account.to_string());
        self.staker = Some(staker.to_string());
        self.sol_deposit_auth = sol_deposit_authority.map(|s| s.to_string());
        let (default_deposit_auth, _bump) = FindDepositAuthority { pool }.run_for_prog(program_id);
        self.stake_deposit_auth = if *stake_deposit_authority != default_deposit_auth {
            Some(stake_deposit_authority.to_string())
        } else {
            None
        };
        self.stake_withdraw_auth = Some(
            FindWithdrawAuthority { pool }
                .run_for_prog(program_id)
                .0
                .to_string(),
        );
        self.sol_withdraw_auth = sol_withdraw_authority.map(|s| s.to_string());
        self.preferred_deposit_validator =
            preferred_deposit_validator_vote_address.map(|pk| pk.to_string());
        self.preferred_withdraw_validator =
            preferred_withdraw_validator_vote_address.map(|pk| pk.to_string());
        self.stake_deposit_referral_fee = Some(*stake_referral_fee);
        self.sol_deposit_referral_fee = Some(*sol_referral_fee);
        self.epoch_fee = Some(epoch_fee.clone());
        self.stake_withdrawal_fee = Some(stake_withdrawal_fee.clone());
        self.sol_withdrawal_fee = Some(sol_withdrawal_fee.clone());
        self.stake_deposit_fee = Some(stake_deposit_fee.clone());
        self.sol_deposit_fee = Some(sol_deposit_fee.clone());
        self.total_lamports = Some(*total_lamports);
        self.pool_token_supply = Some(*pool_token_supply);
        self.last_update_epoch = Some(*last_update_epoch);
        self.next_epoch_fee = Some(next_epoch_fee.clone());
        self.next_stake_withdrawal_fee = Some(next_stake_withdrawal_fee.clone());
        self.next_sol_withdrawal_fee = Some(next_sol_withdrawal_fee.clone());
        self.last_epoch_pool_token_supply = Some(*last_epoch_pool_token_supply);
        self.last_epoch_total_lamports = Some(*last_epoch_total_lamports);
    }

    pub fn set_validator_list(
        &mut self,
        program_id: &Pubkey,
        pool: &Pubkey,
        ValidatorList {
            header: ValidatorListHeader { max_validators, .. },
            validators,
        }: &ValidatorList,
    ) {
        self.max_validators = Some(*max_validators);
        self.validators = Some(
            validators
                .iter()
                .map(|vsi| ValidatorConfigRaw::from_vsi_program_pool(vsi, program_id, pool))
                .collect(),
        )
    }
}
