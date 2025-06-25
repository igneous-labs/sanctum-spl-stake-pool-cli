use clap::{
    builder::{StringValueParser, TypedValueParser},
    Args,
};
use sanctum_associated_token_lib::FindAtaAddressArgs;
use sanctum_solana_cli_utils::{PubkeySrc, TokenAmt, TokenAmtParser, TxSendMode};
use sanctum_spl_stake_pool_lib::FindWithdrawAuthority;
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{
    clock::Clock,
    instruction::{AccountMeta, Instruction},
    system_program, sysvar,
};
use spl_associated_token_account_interface::CreateIdempotentKeys;
use spl_stake_pool_interface::{StakePool, ValidatorList};

use crate::{handle_tx_full, update_pool, with_auto_cb_ixs, Subcmd, UpdateCtrl, UpdatePoolArgs};

#[derive(Args, Debug)]
#[command(
    long_about = "Deposits (unwrapped) SOL from the payer wallet into a stake pool, minting the LST in return"
)]
pub struct DepositSolArgs {
    #[arg(
        long,
        short,
        help = "Token account to receive the minted pool tokens. Defaults to authority's ATA, optionally creating it, if not set."
    )]
    pub mint_to: Option<String>,

    #[arg(
        help = "The stake pool to deposit SOL into. Either its pubkey or the stake pool's keypair."
    )]
    pub pool: String,

    #[arg(
        help = "Amount in SOL to deposit.",
        value_parser = StringValueParser::new().map(|s| TokenAmtParser::new(9).parse(&s).unwrap()),
    )]
    pub sol: TokenAmt,
}

impl DepositSolArgs {
    pub async fn run(args: crate::Args) {
        let Self { mint_to, pool, sol } = match args.subcmd {
            Subcmd::DepositSol(a) => a,
            _ => unreachable!(),
        };

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        let pool = PubkeySrc::parse(&pool).unwrap().pubkey();

        let mut fetched = rpc.get_multiple_accounts(&[pool]).await.unwrap();
        let fetched_pool = fetched.pop().unwrap().unwrap();

        let program_id = fetched_pool.owner;

        let decoded_pool =
            <StakePool as borsh::BorshDeserialize>::deserialize(&mut fetched_pool.data.as_ref())
                .unwrap();

        let (authority_ata, _bump) = FindAtaAddressArgs {
            wallet: payer.pubkey(),
            mint: decoded_pool.pool_mint,
            token_program: decoded_pool.token_program,
        }
        .find_ata_address();
        let mint_to = mint_to
            .map(|s| PubkeySrc::parse(&s).unwrap().pubkey())
            .unwrap_or(authority_ata);
        let is_mint_to_authority_ata = mint_to == authority_ata;

        let fetched = rpc
            .get_multiple_accounts(&[decoded_pool.validator_list, mint_to, sysvar::clock::ID])
            .await
            .unwrap();
        let [fetched_validator_list, maybe_fetched_mint_to, clock]: &[_; 3] =
            fetched.as_slice().try_into().unwrap();

        let Clock {
            epoch: current_epoch,
            ..
        } = bincode::deserialize(&clock.as_ref().unwrap().data).unwrap();

        let ValidatorList { validators, .. } =
            <ValidatorList as borsh::BorshDeserialize>::deserialize(
                &mut fetched_validator_list.as_ref().unwrap().data.as_slice(),
            )
            .unwrap();

        update_pool(UpdatePoolArgs {
            rpc: &rpc,
            send_mode: args.send_mode,
            payer: payer.as_ref(),
            program_id,
            current_epoch,
            stake_pool: Keyed {
                pubkey: pool,
                account: &fetched_pool,
            },
            validator_list_entries: &validators,
            fee_limit_cb: args.fee_limit_cb,
            ctrl: UpdateCtrl::IfNeeded,
            no_merge: false,
        })
        .await;

        let mut ixs = vec![];
        if maybe_fetched_mint_to.is_none() {
            if !is_mint_to_authority_ata {
                panic!("mint_to does not exist and is not authority's ATA");
            } else {
                eprintln!("Will create ATA {mint_to} to receive minted LSTs");
                ixs.push(
                    spl_associated_token_account_interface::create_idempotent_ix(
                        CreateIdempotentKeys {
                            funding_account: payer.pubkey(),
                            associated_token_account: mint_to,
                            wallet: payer.pubkey(),
                            mint: decoded_pool.pool_mint,
                            system_program: system_program::ID,
                            token_program: decoded_pool.token_program,
                        },
                    )
                    .unwrap(),
                )
            }
        }

        // manually craft deposit sol instruction here because i fukt up
        // and wrote sanctum_spl_stake_pool_lib into unresolvable dependency hell
        let mut data = vec![14];
        data.extend_from_slice(&sol.amt.to_le_bytes());
        let mut accounts = vec![
            AccountMeta {
                pubkey: pool,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: FindWithdrawAuthority { pool }.run_for_prog(&program_id).0,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: decoded_pool.reserve_stake,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: payer.pubkey(),
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: mint_to,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: decoded_pool.manager_fee_account,
                is_signer: false,
                is_writable: true,
            },
            // referrer
            AccountMeta {
                pubkey: mint_to,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: decoded_pool.pool_mint,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: system_program::ID,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: decoded_pool.token_program,
                is_signer: false,
                is_writable: false,
            },
        ];
        if let Some(deposit_auth) = decoded_pool.sol_deposit_authority {
            accounts.push(AccountMeta {
                pubkey: deposit_auth,
                is_signer: true,
                is_writable: false,
            });
        }
        ixs.push(Instruction {
            program_id,
            data,
            accounts,
        });

        // TODO: calc expected amount after fees
        eprintln!("Depositing {sol} SOL");
        let ixs = match args.send_mode {
            TxSendMode::DumpMsg => ixs,
            _ => with_auto_cb_ixs(&rpc, &payer.pubkey(), ixs, &[], args.fee_limit_cb).await,
        };
        let mut signers = [payer.as_ref()];
        handle_tx_full(&rpc, args.send_mode, &ixs, &[], &mut signers).await;
    }
}
