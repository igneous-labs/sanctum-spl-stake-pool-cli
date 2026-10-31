use std::{num::NonZeroU32, path::PathBuf, str::FromStr};

use borsh::BorshDeserialize;
use clap::Args;
use sanctum_associated_token_lib::FindAtaAddressArgs;
use sanctum_solana_cli_utils::{PubkeySrc, TokenAmt, TxSendMode};
use sanctum_spl_stake_pool_lib::{FindValidatorStakeAccount, FindValidatorStakeAccountArgs};
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{
    clock::Clock, instruction::Instruction, pubkey::Pubkey, rent::Rent, system_program, sysvar,
};
use spl_associated_token_account_interface::CreateIdempotentKeys;
use spl_stake_pool_interface::{StakePool, ValidatorList};

use crate::{
    pool_config::{
        print_adding_validators_msg, print_removing_validators_msg, utils::lamports_for_new_vsa,
        ConfigRaw, SyncValidatorListConfig,
    },
    ps,
    tx_utils::{
        get_multiple_accounts_chunked, handle_tx_full, with_auto_cb_ixs,
        MAX_ACCOUNTS_PER_GET_MULTIPLE_ACCOUNTS, MAX_ADD_VALIDATORS_IX_PER_TX,
        MAX_REMOVE_VALIDATOR_IXS_ENUM_PER_TX,
    },
    update::{update_pool, UpdatePoolArgs},
    UpdateCtrl,
};

use super::{deposit_sol_ix, Subcmd};

#[derive(Args, Debug)]
#[command(
    long_about = "(Staker only) sync validator list entries and preferred validators with a pool config file"
)]
pub struct SyncValidatorListArgs {
    #[arg(
        short,
        long,
        help = "Prepend a DepositSol instruction to each add/remove validator transaction, funding the lamports required to be transferred from the pool's reserve to create the stake accounts (VSA/TSA/ephemeral) for that transaction. Deposits from the config payer and mints LSTs to the payer's associated token account."
    )]
    pub deposit_sol_for_rent: bool,

    #[arg(
        help = "Path to pool config file containing the updated validator list and preferred validators to update the pool to"
    )]
    pub pool_config: PathBuf,
}

impl SyncValidatorListArgs {
    pub async fn run(args: crate::Args) {
        let Self {
            pool_config,
            deposit_sol_for_rent,
        } = match args.subcmd {
            Subcmd::SyncValidatorList(a) => a,
            _ => unreachable!(),
        };

        let ConfigRaw {
            preferred_deposit_validator,
            preferred_withdraw_validator,
            validators,
            pool,
            staker,
            ..
        } = ConfigRaw::read_from_path(pool_config).unwrap();

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        ps!(staker, @fb payer.as_ref(), @sm args.send_mode);

        let [preferred_deposit_validator, preferred_withdraw_validator] =
            [preferred_deposit_validator, preferred_withdraw_validator]
                .map(|opt| opt.map(|s| PubkeySrc::parse(&s).unwrap().pubkey()));

        let pool = PubkeySrc::parse(pool.as_ref().unwrap()).unwrap().pubkey();

        let mut fetched = rpc
            .get_multiple_accounts(&[pool, sysvar::clock::ID, sysvar::rent::ID])
            .await
            .unwrap();
        let rent = fetched.pop().unwrap().unwrap();
        let clock = fetched.pop().unwrap().unwrap();
        let stake_pool_acc = fetched.pop().unwrap().unwrap();
        let program_id = stake_pool_acc.owner;

        let rent: Rent = bincode::deserialize(&rent.data).unwrap();
        let Clock { epoch, .. } = bincode::deserialize(&clock.data).unwrap();
        let stake_pool = StakePool::deserialize(&mut stake_pool_acc.data.as_slice()).unwrap();

        let validator_list_acc = rpc.get_account(&stake_pool.validator_list).await.unwrap();
        let ValidatorList {
            validators: old_validators,
            ..
        } = ValidatorList::deserialize(&mut validator_list_acc.data.as_slice()).unwrap();

        // need to update first to be able to add/remove validators
        update_pool(UpdatePoolArgs {
            rpc: &rpc,
            send_mode: args.send_mode,
            payer: payer.as_ref(),
            program_id,
            current_epoch: epoch,
            stake_pool: Keyed {
                pubkey: pool,
                account: &stake_pool_acc,
            },
            validator_list_entries: &old_validators,
            fee_limit_cb: args.fee_limit_cb,
            ctrl: UpdateCtrl::IfNeeded,
            no_merge: false,
        })
        .await;

        let svlc = SyncValidatorListConfig {
            program_id,
            payer: payer.as_ref(),
            staker,
            pool,
            validator_list: stake_pool.validator_list,
            reserve: stake_pool.reserve_stake,
            preferred_deposit_validator,
            preferred_withdraw_validator,
            validators: validators
                .unwrap_or_default()
                .into_iter()
                .map(|v| Pubkey::from_str(&v.vote).unwrap())
                .collect(),
            rent: &rent,
        };

        let deposit_sol_for_rent = if deposit_sol_for_rent {
            let from = payer.pubkey();
            if let Some(sol_deposit_authority) = stake_pool.sol_deposit_authority {
                if sol_deposit_authority != payer.pubkey()
                    && sol_deposit_authority != staker.pubkey()
                {
                    panic!(
                        "Pool requires SOL deposit authority {sol_deposit_authority} to sign, but it is neither the payer nor staker"
                    );
                }
            }
            let (mint_to, _bump) = FindAtaAddressArgs {
                wallet: from,
                mint: stake_pool.pool_mint,
                token_program: stake_pool.token_program,
            }
            .find_ata_address();
            let create_ata_ix = if rpc.get_account(&mint_to).await.is_ok() {
                None
            } else {
                eprintln!("Will create ATA {mint_to} to receive minted LSTs");
                Some(
                    spl_associated_token_account_interface::create_idempotent_ix(
                        CreateIdempotentKeys {
                            funding_account: payer.pubkey(),
                            associated_token_account: mint_to,
                            wallet: from,
                            mint: stake_pool.pool_mint,
                            system_program: system_program::ID,
                            token_program: stake_pool.token_program,
                        },
                    )
                    .unwrap(),
                )
            };
            Some((from, mint_to, create_ata_ix))
        } else {
            None
        };

        let prefund_ixs = |lamports: u64| -> Vec<Instruction> {
            let Some((from, mint_to, create_ata_ix)) = &deposit_sol_for_rent else {
                return vec![];
            };
            if lamports == 0 {
                return vec![];
            }
            eprintln!(
                "Prepending deposit of {} SOL to fund stake account rent",
                TokenAmt {
                    amt: lamports,
                    decimals: 9
                }
            );
            let mut ixs = Vec::with_capacity(2);
            if let Some(create_ata_ix) = create_ata_ix {
                ixs.push(create_ata_ix.clone());
            }
            ixs.push(deposit_sol_ix(
                program_id,
                pool,
                &stake_pool,
                *from,
                *mint_to,
                lamports,
            ));
            ixs
        };

        let (add, remove) = svlc.add_remove_changeset(&old_validators);
        // need to additionally fetch VSAs of validators to remove to make sure they weren't
        // already DeactivateDelinquent'd
        let remove_vsas: Vec<Pubkey> = remove
            .clone()
            .map(|vsi| {
                FindValidatorStakeAccount::new(FindValidatorStakeAccountArgs {
                    pool,
                    vote: vsi.vote_account_address,
                    seed: NonZeroU32::new(vsi.validator_seed_suffix),
                })
                .run_for_prog(&program_id)
                .0
            })
            .collect();
        let remove_vsas = get_multiple_accounts_chunked(
            &rpc,
            &remove_vsas,
            MAX_ACCOUNTS_PER_GET_MULTIPLE_ACCOUNTS,
        )
        .await
        .into_iter()
        .map(|acc_opt| bincode::deserialize(&acc_opt.unwrap().data).unwrap());

        print_removing_validators_msg(remove.clone());

        for remove_validator_ix_chunk in svlc
            .remove_validators_ixs_enum(remove.zip(remove_vsas))
            .unwrap()
            .as_slice()
            .chunks(MAX_REMOVE_VALIDATOR_IXS_ENUM_PER_TX)
        {
            let reserve_lamports = remove_validator_ix_chunk
                .iter()
                .map(|remove_ixs| remove_ixs.reserve_rent_lamports(&rent))
                .sum();
            let mut remove_validator_ix_chunk: Vec<Instruction> = remove_validator_ix_chunk
                .iter()
                .cloned()
                .flat_map(IntoIterator::into_iter)
                .collect();
            let mut prefund_ixs = prefund_ixs(reserve_lamports);
            prefund_ixs.append(&mut remove_validator_ix_chunk);
            let remove_validator_ix_chunk = match args.send_mode {
                TxSendMode::DumpMsg => prefund_ixs,
                _ => {
                    with_auto_cb_ixs(&rpc, &payer.pubkey(), prefund_ixs, &[], args.fee_limit_cb)
                        .await
                }
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &remove_validator_ix_chunk,
                &[],
                &mut svlc.signers_maybe_dup(),
            )
            .await;
        }

        print_adding_validators_msg(add.clone());

        for add_validator_ix_chunk in svlc
            .add_validators_ixs(add)
            .unwrap()
            .as_slice()
            .chunks(MAX_ADD_VALIDATORS_IX_PER_TX)
        {
            let reserve_lamports = u64::try_from(add_validator_ix_chunk.len())
                .unwrap()
                .saturating_mul(lamports_for_new_vsa(&rent));
            let mut prefund_ixs = prefund_ixs(reserve_lamports);
            prefund_ixs.extend_from_slice(add_validator_ix_chunk);
            let add_validator_ix_chunk = match args.send_mode {
                TxSendMode::DumpMsg => prefund_ixs,
                _ => {
                    with_auto_cb_ixs(&rpc, &payer.pubkey(), prefund_ixs, &[], args.fee_limit_cb)
                        .await
                }
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &add_validator_ix_chunk,
                &[],
                &mut svlc.signers_maybe_dup(),
            )
            .await;
        }

        let preferred_validator_changes = svlc.preferred_validator_changeset(&stake_pool);
        for change in preferred_validator_changes.clone() {
            eprintln!("{change}");
        }
        let preferred_validator_ixs = svlc
            .preferred_validator_ixs(preferred_validator_changes)
            .unwrap();
        if !preferred_validator_ixs.is_empty() {
            let preferred_validator_ixs = match args.send_mode {
                TxSendMode::DumpMsg => preferred_validator_ixs,
                _ => {
                    with_auto_cb_ixs(
                        &rpc,
                        &payer.pubkey(),
                        preferred_validator_ixs,
                        &[],
                        args.fee_limit_cb,
                    )
                    .await
                }
            };
            handle_tx_full(
                &rpc,
                args.send_mode,
                &preferred_validator_ixs,
                &[],
                &mut svlc.signers_maybe_dup(),
            )
            .await;
        }
    }
}
