use clap::{
    builder::{StringValueParser, TypedValueParser},
    Args,
};
use rand::Rng;
use sanctum_associated_token_lib::FindAtaAddressArgs;
use sanctum_solana_cli_utils::{
    PubkeySrc, TokenAmt, TokenAmtOrAll, TokenAmtOrAllParser, TxSendMode,
};
use sanctum_spl_stake_pool_lib::account_resolvers::WithdrawStakeWithSlippage;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{
    clock::Clock,
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    rent::Rent,
    stake::{self, state::StakeStateV2},
    system_instruction, sysvar,
};
use spl_stake_pool_interface::{
    withdraw_stake_with_slippage_ix_with_program_id, StakePool, ValidatorList,
    WithdrawStakeWithSlippageIxArgs,
};

use crate::{
    handle_tx_full, parse_signer_allow_pubkey, update_pool, with_auto_cb_ixs, Subcmd, UpdateCtrl,
    UpdatePoolArgs,
};

#[derive(Args, Debug)]
#[command(long_about = "Withdraws stake from a stake pool")]
pub struct WithdrawStakeArgs {
    #[arg(
        long,
        short,
        help = "Token account authority of burn_from. Defaults to payer if not set."
    )]
    pub authority: Option<String>,

    #[arg(
        long,
        short,
        help = "Token account to burn and redeem pool tokens from. Defaults to authority's ATA if not set."
    )]
    pub tokens_burn_from: Option<String>,

    #[arg(
        long,
        short,
        help = "Account receiving the stake account. Defaults to authority if not set."
    )]
    pub beneficiary: Option<String>,

    #[arg(
        long,
        short,
        help = "Validator vote account of stake account to withdraw to. Defaults to the pool's largest validator if not set. Cannot be used if pool has preferred validator"
    )]
    pub validator: Option<String>,

    #[arg(
        help = "The stake pool to withdraw stake from. Either the stake pool's pubkey or keypair."
    )]
    pub pool: String,

    #[arg(
        help = "Amount of stake pool tokens to redeem. Also accepts 'all'.",
        value_parser = StringValueParser::new().map(|s| TokenAmtOrAllParser::new(9).parse(&s).unwrap()),
    )]
    pub token_amt: TokenAmtOrAll,
}

impl WithdrawStakeArgs {
    pub async fn run(args: crate::Args) {
        let Self {
            authority,
            tokens_burn_from,
            beneficiary,
            pool,
            validator,
            token_amt,
        } = match args.subcmd {
            Subcmd::WithdrawStake(a) => a,
            _ => unreachable!(),
        };

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        // allow pubkey signers to work with multisig programs
        let authority = authority.map(|s| parse_signer_allow_pubkey(&s).unwrap());
        let authority = authority
            .as_ref()
            .map_or_else(|| payer.as_ref(), |authority| authority.as_ref());
        let beneficiary = beneficiary.map_or_else(
            || authority.pubkey(),
            |b| PubkeySrc::parse(&b).unwrap().pubkey(),
        );
        let pool = PubkeySrc::parse(&pool).unwrap().pubkey();
        let validator = validator.map(|s| PubkeySrc::parse(&s).unwrap().pubkey());

        let fetched_pool = rpc.get_account(&pool).await.unwrap();
        let program_id = fetched_pool.owner;
        let decoded_pool =
            <StakePool as borsh::BorshDeserialize>::deserialize(&mut fetched_pool.data.as_ref())
                .unwrap();

        let burn_from = tokens_burn_from.map_or_else(
            || {
                FindAtaAddressArgs {
                    wallet: authority.pubkey(),
                    mint: decoded_pool.pool_mint,
                    token_program: decoded_pool.token_program,
                }
                .find_ata_address()
                .0
            },
            |b| PubkeySrc::parse(&b).unwrap().pubkey(),
        );

        let mut fetched = rpc
            .get_multiple_accounts(&[burn_from, decoded_pool.validator_list])
            .await
            .unwrap();

        let fetched_validator_list = fetched.pop().unwrap().unwrap();
        let ValidatorList { validators, .. } =
            <ValidatorList as borsh::BorshDeserialize>::deserialize(
                &mut fetched_validator_list.data.as_slice(),
            )
            .unwrap();

        let fetched_burn_from = fetched.pop().unwrap().unwrap();
        let decoded_burn_from = spl_token_2022::extension::StateWithExtensions::<
            spl_token_2022::state::Account,
        >::unpack(&fetched_burn_from.data)
        .unwrap()
        .base;
        let amt = match token_amt {
            TokenAmtOrAll::All { .. } => decoded_burn_from.amount,
            TokenAmtOrAll::Amt { amt, .. } => {
                if amt > decoded_burn_from.amount {
                    panic!(
                        "Insufficient balance in burn_from. Requested {}, has {}",
                        token_amt,
                        TokenAmt {
                            amt: decoded_burn_from.amount,
                            decimals: 9
                        }
                    )
                }
                amt
            }
        };

        // TODO: handle tsa and reserve edge cases
        // TODO make sure validator has enough stake to service withdrawal
        let vsi = match decoded_pool.preferred_withdraw_validator_vote_address {
            Some(preferred) => {
                if let Some(v) = validator {
                    if v != preferred {
                        panic!("Want to withdraw from validator {v} but stake pool's preferred is {preferred}");
                    }
                }
                validators
                    .iter()
                    .find(|vsi| vsi.vote_account_address == preferred)
                    .unwrap()
            }
            None => validator.map_or_else(
                || {
                    validators
                        .iter()
                        .max_by_key(|vsi| vsi.active_stake_lamports)
                        .expect("No validators in pool")
                },
                |v| {
                    validators
                        .iter()
                        .find(|vsi| vsi.vote_account_address == v)
                        .unwrap_or_else(|| panic!("Validator {v} not part of pool"))
                },
            ),
        };

        let (split_to, seed) =
            find_unused_stake_prog_create_with_seed(&rpc, &authority.pubkey()).await;

        let mut fetched = rpc
            .get_multiple_accounts(&[sysvar::clock::ID, sysvar::rent::ID])
            .await
            .unwrap();
        let rent = fetched.pop().unwrap().unwrap();
        let rent: Rent = bincode::deserialize(&rent.data).unwrap();
        let clock = fetched.pop().unwrap().unwrap();
        let Clock {
            epoch: current_epoch,
            ..
        } = bincode::deserialize(&clock.data).unwrap();

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
        })
        .await;

        // TODO: calc expected amount after fees
        eprintln!(
            "Redeeming {} tokens for stake account staked to validator {}",
            token_amt, vsi.vote_account_address
        );
        let resolve = WithdrawStakeWithSlippage {
            pool: Keyed {
                pubkey: pool,
                account: &decoded_pool,
            },
            burn_from,
            transfer_authority: authority.pubkey(),
            beneficiary,
            split_to,
        };
        let computed_keys = resolve.compute_keys_for_vsa(
            &program_id,
            vsi.vote_account_address,
            vsi.validator_seed_suffix,
        );
        let mut signers = [payer.as_ref(), authority];
        let ixs = vec![
            system_instruction::create_account_with_seed(
                &payer.pubkey(),
                &split_to,
                &authority.pubkey(),
                &seed,
                rent.minimum_balance(StakeStateV2::size_of()),
                StakeStateV2::size_of() as u64,
                &stake::program::ID,
            ),
            withdraw_stake_with_slippage_ix_with_program_id(
                program_id,
                resolve.resolve_with_computed_keys(computed_keys),
                WithdrawStakeWithSlippageIxArgs {
                    pool_tokens_in: amt,
                    min_lamports_out: 0, // TODO: slippage
                },
            )
            .unwrap(),
        ];
        let ixs = match args.send_mode {
            TxSendMode::DumpMsg => ixs,
            _ => with_auto_cb_ixs(&rpc, &payer.pubkey(), ixs, &[], args.fee_limit_cb).await,
        };
        handle_tx_full(&rpc, args.send_mode, &ixs, &[], &mut signers).await;
    }
}

async fn find_unused_stake_prog_create_with_seed(
    rpc: &RpcClient,
    authority: &Pubkey,
) -> (Pubkey, String) {
    // MAX_SEED_LEN = 32, just randomly generate u32 as string to make seed
    const MAX_ATTEMPTS: usize = 5;
    let mut rng = rand::thread_rng();
    for _i in 0..MAX_ATTEMPTS {
        let seed: u32 = rng.gen();
        let seed = seed.to_string();
        let pk = Pubkey::create_with_seed(authority, &seed, &stake::program::ID).unwrap();
        let acc = rpc
            .get_account_with_commitment(&pk, CommitmentConfig::processed())
            .await
            .unwrap();
        if acc.value.is_none() {
            return (pk, seed);
        }
    }
    panic!("Could not find unused seed for new stake account");
}
