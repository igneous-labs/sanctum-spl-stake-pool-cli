use std::{error::Error, path::PathBuf, str::FromStr};

use clap::Args;
use sanctum_associated_token_lib::FindAtaAddressArgs;
use sanctum_solana_cli_utils::{parse_pubkey_src, parse_signer, TxSendMode};
use sanctum_spl_stake_pool_lib::FindDepositAuthority;
use solana_readonly_account::{keyed::Keyed, ReadonlyAccountOwner};
use solana_sdk::{
    account::Account, program_option::COption, pubkey::Pubkey, rent::Rent, system_program, sysvar,
};
use spl_associated_token_account_interface::CreateIdempotentKeys;
use spl_stake_pool_interface::{AccountType, FutureEpochFee, Lockup, StakePool};
use spl_token_2022::{extension::StateWithExtensions, state::Mint};

use crate::{
    consts::ZERO_FEE,
    pool_config::{ConfigFileRaw, CreateConfig, SyncPoolManagerConfig},
    subcmd::Subcmd,
    tx_utils::{handle_tx_full, with_auto_cb_ixs},
    utils::filter_default_stake_deposit_auth,
};

#[derive(Args, Debug)]
#[command(long_about = "Create a new stake pool")]
pub struct CreatePoolArgs {
    #[arg(help = "Path to pool config file")]
    pub pool_config: PathBuf,
}

impl CreatePoolArgs {
    pub async fn run(args: crate::Args) {
        let Self { pool_config } = match args.subcmd {
            Subcmd::CreatePool(a) => a,
            _ => unreachable!(),
        };

        let ConfigFileRaw {
            mint,
            pool,
            validator_list,
            reserve,
            manager,
            manager_fee_account,
            staker,
            stake_deposit_auth,
            max_validators,
            stake_deposit_referral_fee,
            sol_deposit_referral_fee,
            epoch_fee,
            stake_withdrawal_fee,
            sol_withdrawal_fee,
            stake_deposit_fee,
            sol_deposit_fee,
            sol_deposit_auth,
            sol_withdraw_auth,
            ..
        } = ConfigFileRaw::read_from_path(pool_config).unwrap();

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let [pool, validator_list, reserve] =
            [pool.as_ref(), validator_list.as_ref(), reserve.as_ref()]
                .map(|p| parse_signer(p.unwrap()).unwrap());
        let manager = manager.map(|m| parse_signer(&m).unwrap());
        let manager = manager
            .as_ref()
            .map(|m| m.as_ref())
            .unwrap_or(payer.as_ref());
        let mint = Pubkey::from_str(&mint.unwrap()).unwrap();

        let mut fetched = rpc
            .get_multiple_accounts(&[sysvar::rent::ID, mint])
            .await
            .unwrap();
        let mint_acc = fetched.pop().unwrap();

        let rent = fetched.pop().unwrap().unwrap();
        let rent: Rent = bincode::deserialize(&rent.data).unwrap();

        let mint_acc = mint_acc.expect("Mint not initialized");
        verify_mint(&mint_acc, &manager.pubkey()).unwrap();

        let manager_fee_ata = FindAtaAddressArgs {
            wallet: manager.pubkey(),
            mint,
            token_program: mint_acc.owner,
        }
        .find_ata_address()
        .0;
        let manager_fee_account = manager_fee_account
            .map(|s| Pubkey::from_str(&s).unwrap())
            .unwrap_or(manager_fee_ata);
        let manager_fee_fetched = rpc
            .get_multiple_accounts(&[manager_fee_account])
            .await
            .unwrap()
            .pop()
            .unwrap();

        let (default_stake_deposit_auth, _bump) = FindDepositAuthority {
            pool: pool.pubkey(),
        }
        .run_for_prog(&args.program);

        let cc = CreateConfig {
            mint: Keyed {
                pubkey: mint,
                account: &mint_acc,
            },
            program_id: args.program,
            payer: payer.as_ref(),
            pool: pool.as_ref(),
            validator_list: validator_list.as_ref(),
            reserve: reserve.as_ref(),
            manager,
            manager_fee_account,
            staker: staker.map_or_else(|| manager.pubkey(), |s| Pubkey::from_str(&s).unwrap()),
            deposit_auth: stake_deposit_auth.map_or_else(
                || None,
                |s| {
                    filter_default_stake_deposit_auth(
                        Pubkey::from_str(&s).unwrap(),
                        &default_stake_deposit_auth,
                    )
                },
            ),
            deposit_referral_fee: stake_deposit_referral_fee
                .or(sol_deposit_referral_fee)
                .unwrap_or_default(),
            epoch_fee: epoch_fee.unwrap_or(ZERO_FEE),
            withdrawal_fee: stake_withdrawal_fee
                .or(sol_withdrawal_fee.clone())
                .unwrap_or(ZERO_FEE),
            deposit_fee: stake_deposit_fee
                .or(sol_deposit_fee.clone())
                .unwrap_or(ZERO_FEE),
            max_validators: max_validators.unwrap(),
            rent,
        };

        let mut first_ixs = Vec::from(cc.create_reserve_tx_ixs().unwrap());

        if manager_fee_fetched.is_none() {
            if manager_fee_account != manager_fee_ata {
                panic!("Manager fee account does not exist and is not ATA");
            }
            eprintln!("Creating manager fee account {manager_fee_account}");
            first_ixs.push(
                spl_associated_token_account_interface::create_idempotent_ix(
                    CreateIdempotentKeys {
                        funding_account: payer.pubkey(),
                        associated_token_account: manager_fee_account,
                        wallet: manager.pubkey(),
                        mint,
                        system_program: system_program::ID,
                        token_program: mint_acc.owner,
                    },
                )
                .unwrap(),
            );
        }
        let first_ixs = match args.send_mode {
            TxSendMode::DumpMsg => first_ixs,
            _ => with_auto_cb_ixs(&rpc, &payer.pubkey(), first_ixs, &[], args.fee_limit_cu).await,
        };
        handle_tx_full(
            &rpc,
            args.send_mode,
            &first_ixs,
            &[],
            &mut cc.create_reserve_tx_signers_maybe_dup(),
        )
        .await;

        let ixs = Vec::from(cc.initialize_tx_ixs().unwrap());
        let ixs = match args.send_mode {
            TxSendMode::DumpMsg => ixs,
            _ => with_auto_cb_ixs(&rpc, &payer.pubkey(), ixs, &[], args.fee_limit_cu).await,
        };
        handle_tx_full(
            &rpc,
            args.send_mode,
            &ixs,
            &[],
            &mut cc.initialize_tx_signers_maybe_dup(),
        )
        .await;

        // use a dummy instead of fetching the newly created pool from rpc so that it works for --dump-msg
        //
        // the sol_*_fees are going to be set to the same value as the
        // the stake_*_fees during initialization
        //
        // sol deposit authority is set to stake deposit authority (last optional account) during initialization
        //
        // sol withdraw authority is set to None during initialization
        let created_dummy = StakePool {
            account_type: AccountType::StakePool,
            manager: cc.manager.pubkey(),
            staker: cc.staker,
            stake_deposit_authority: cc.deposit_auth.unwrap_or(default_stake_deposit_auth),
            validator_list: cc.validator_list.pubkey(),
            reserve_stake: cc.reserve.pubkey(),
            pool_mint: cc.mint.pubkey,
            manager_fee_account,
            token_program_id: *cc.mint.owner(),
            epoch_fee: cc.epoch_fee.clone(),
            next_epoch_fee: FutureEpochFee::None,
            stake_deposit_fee: cc.deposit_fee.clone(),
            stake_withdrawal_fee: cc.withdrawal_fee.clone(),
            next_stake_withdrawal_fee: FutureEpochFee::None,
            stake_referral_fee: cc.deposit_referral_fee,
            sol_deposit_authority: cc.deposit_auth,
            sol_deposit_fee: cc.deposit_fee.clone(),
            sol_referral_fee: cc.deposit_referral_fee,
            sol_withdraw_authority: None,
            sol_withdrawal_fee: cc.withdrawal_fee.clone(),
            next_sol_withdrawal_fee: FutureEpochFee::None,
            // dont cares:
            preferred_deposit_validator_vote_address: None,
            preferred_withdraw_validator_vote_address: None,
            lockup: Lockup {
                unix_timestamp: 0,
                epoch: 0,
                custodian: Pubkey::default(),
            },
            total_lamports: 0,
            pool_token_supply: 0,
            last_update_epoch: 0,
            stake_withdraw_bump_seed: 255,
            last_epoch_pool_token_supply: 0,
            last_epoch_total_lamports: 0,
        };
        let spmc = SyncPoolManagerConfig {
            program_id: args.program,
            pool: cc.pool.pubkey(),
            payer: cc.payer,
            manager,
            new_manager: manager,
            staker: cc.staker,
            manager_fee_account,
            sol_deposit_auth: sol_deposit_auth.map(|s| parse_pubkey_src(&s).unwrap().pubkey()),
            stake_deposit_auth: cc.deposit_auth,
            sol_withdraw_auth: sol_withdraw_auth.map(|s| parse_pubkey_src(&s).unwrap().pubkey()),
            epoch_fee: cc.epoch_fee,
            stake_deposit_referral_fee: cc.deposit_referral_fee,
            sol_deposit_referral_fee: sol_deposit_referral_fee.unwrap_or(0),
            stake_withdrawal_fee: cc.withdrawal_fee,
            sol_withdrawal_fee: sol_withdrawal_fee.unwrap_or(ZERO_FEE),
            stake_deposit_fee: cc.deposit_fee,
            sol_deposit_fee: sol_deposit_fee.unwrap_or(ZERO_FEE),
        };

        let changeset = spmc.changeset(&created_dummy);
        for change in changeset.iter() {
            eprintln!("{change}");
        }
        let sync_pool_ixs = spmc.changeset_ixs(&changeset).unwrap();
        let sync_pool_ixs = match args.send_mode {
            TxSendMode::DumpMsg => sync_pool_ixs,
            _ => {
                with_auto_cb_ixs(&rpc, &payer.pubkey(), sync_pool_ixs, &[], args.fee_limit_cu).await
            }
        };
        handle_tx_full(
            &rpc,
            args.send_mode,
            &sync_pool_ixs,
            &[],
            &mut spmc.signers_maybe_dup(),
        )
        .await;
    }
}

fn verify_mint(mint: &Account, manager_pk: &Pubkey) -> Result<(), Box<dyn Error>> {
    let StateWithExtensions { base: mint, .. } = StateWithExtensions::<Mint>::unpack(&mint.data)?;
    if mint.decimals != 9 {
        return Err("Mint not of 9 decimals".into());
    }
    if mint.freeze_authority.is_some() {
        return Err("Mint has freeze authority".into());
    }
    if mint.supply > 0 {
        return Err("Mint has nonzero supply".into());
    }
    match mint.mint_authority {
        COption::None => return Err("Mint has no mint authority".into()),
        COption::Some(mint_auth) => {
            if mint_auth != *manager_pk {
                return Err("Mint authority not set to manager".into());
            }
        }
    }
    // TODO: verify acceptable extensions
    Ok(())
}
