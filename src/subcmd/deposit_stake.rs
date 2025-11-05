use borsh::BorshDeserialize;
use clap::Args;
use sanctum_associated_token_lib::FindAtaAddressArgs;
use sanctum_solana_cli_utils::{PubkeySrc, TxSendMode};
use sanctum_spl_stake_pool_lib::account_resolvers::DepositStakeWithSlippage;
use solana_readonly_account::keyed::Keyed;
use solana_sdk::{
    clock::Clock,
    stake::state::{Authorized, StakeStateV2},
    system_program, sysvar,
};
use spl_associated_token_account_interface::CreateIdempotentKeys;
use spl_stake_pool_interface::{StakePool, ValidatorList, ValidatorStakeInfo};

use crate::{
    handle_tx_full, ps, update_pool, with_auto_cb_ixs, Subcmd, UpdateCtrl, UpdatePoolArgs,
};

#[derive(Args, Debug)]
#[command(long_about = "Deposit an activated stake account into a stake pool")]
pub struct DepositStakeArgs {
    #[arg(
        long,
        short,
        help = "Authority of the stake account to deposit. Defaults to payer if not set."
    )]
    pub authority: Option<String>,

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

    #[arg(help = "Stake account to deposit. Either its pubkey or the stake account's keypair.")]
    pub stake_account: String,
}

impl DepositStakeArgs {
    pub async fn run(args: crate::Args) {
        let Self {
            mint_to,
            pool,
            stake_account,
            authority,
        } = match args.subcmd {
            Subcmd::DepositStake(a) => a,
            _ => unreachable!(),
        };

        let rpc = args.config.nonblocking_rpc_client();
        let payer = args.config.signer();

        ps!(authority, @fb payer.as_ref(), @sm args.send_mode);

        let [pool, stake_account] =
            [pool, stake_account].map(|s| PubkeySrc::parse(&s).unwrap().pubkey());

        let mut fetched = rpc
            .get_multiple_accounts(&[pool, stake_account])
            .await
            .unwrap();
        let fetched_stake_account = fetched.pop().unwrap().unwrap();
        let fetched_pool = fetched.pop().unwrap().unwrap();

        let program_id = fetched_pool.owner;

        let decoded_pool =
            <StakePool as borsh::BorshDeserialize>::deserialize(&mut fetched_pool.data.as_ref())
                .unwrap();
        let validator_list_pk = decoded_pool.validator_list;
        let decoded_stake_account =
            StakeStateV2::deserialize(&mut fetched_stake_account.data.as_slice()).unwrap();
        let delegation = match decoded_stake_account.delegation() {
            Some(d) => d,
            None => panic!("Stake account not delegated"),
        };
        let voter = delegation.voter_pubkey;

        if let Some(preferred) = decoded_pool.preferred_deposit_validator_vote_address {
            if preferred != voter {
                panic!("Stake account not staked to preferred voter {preferred}");
            }
        }

        let Authorized { staker, withdrawer } = decoded_stake_account.authorized().unwrap();
        if staker != authority.pubkey() || withdrawer != authority.pubkey() {
            panic!("Stake account not owned by authority");
        }

        let (authority_ata, _bump) = FindAtaAddressArgs {
            wallet: authority.pubkey(),
            mint: decoded_pool.pool_mint,
            token_program: decoded_pool.token_program,
        }
        .find_ata_address();
        let mint_to = mint_to
            .map(|s| PubkeySrc::parse(&s).unwrap().pubkey())
            .unwrap_or(authority_ata);
        let is_mint_to_authority_ata = mint_to == authority_ata;

        let mut fetched = rpc
            .get_multiple_accounts(&[validator_list_pk, authority_ata, sysvar::clock::ID])
            .await
            .unwrap();

        let clock = fetched.pop().unwrap().unwrap();
        let Clock {
            epoch: current_epoch,
            ..
        } = bincode::deserialize(&clock.data).unwrap();

        let maybe_fetched_authority_ata = fetched.pop().unwrap();

        let mut ixs = vec![];
        if maybe_fetched_authority_ata.is_none() {
            if !is_mint_to_authority_ata {
                panic!("mint_to does not exist and is not authority's ATA");
            } else {
                eprintln!("Will create ATA {mint_to} to receive minted LSTs");
                ixs.push(
                    spl_associated_token_account_interface::create_idempotent_ix(
                        CreateIdempotentKeys {
                            funding_account: payer.pubkey(),
                            associated_token_account: mint_to,
                            wallet: authority.pubkey(),
                            mint: decoded_pool.pool_mint,
                            system_program: system_program::ID,
                            token_program: decoded_pool.token_program,
                        },
                    )
                    .unwrap(),
                )
            }
        }

        let fetched_validator_list = fetched.pop().unwrap().unwrap();

        let ValidatorList { validators, .. } =
            <ValidatorList as borsh::BorshDeserialize>::deserialize(
                &mut fetched_validator_list.data.as_slice(),
            )
            .unwrap();

        let ValidatorStakeInfo {
            validator_seed_suffix,
            vote_account_address,
            ..
        } = validators
            .iter()
            .find(|v| v.vote_account_address == voter)
            .expect("Validator not part of stake pool");

        let deposit_stake_accounts = DepositStakeWithSlippage {
            pool: Keyed {
                pubkey: pool,
                account: &decoded_pool,
            },
            stake_depositing: Keyed {
                pubkey: stake_account,
                account: &decoded_stake_account,
            },
            mint_to,
            referral_fee_dest: mint_to,
        };

        ixs.extend(
            deposit_stake_accounts
                .full_ix_seq(
                    &program_id,
                    *vote_account_address,
                    *validator_seed_suffix,
                    0,
                ) // TODO: min_token_outs = 0 right now, need to handle slippage
                .unwrap(),
        );

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

        // TODO: calc expected amount after fees
        eprintln!("Depositing stake account {stake_account}");
        let ixs = match args.send_mode {
            TxSendMode::DumpMsg => ixs,
            _ => with_auto_cb_ixs(&rpc, &payer.pubkey(), ixs, &[], args.fee_limit_cb).await,
        };
        let mut signers = [payer.as_ref(), authority];
        handle_tx_full(&rpc, args.send_mode, &ixs, &[], &mut signers).await;
    }
}
