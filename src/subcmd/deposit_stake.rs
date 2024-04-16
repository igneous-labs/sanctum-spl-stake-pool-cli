use borsh::BorshDeserialize;
use clap::Args;
use sanctum_associated_token_lib::FindAtaAddressArgs;
use sanctum_solana_cli_utils::{parse_signer, PubkeySrc};
use sanctum_spl_stake_pool_lib::FindWithdrawAuthority;
use solana_sdk::{
    stake::{
        self,
        state::{Authorized, StakeAuthorize, StakeStateV2},
    },
    system_program,
};
use spl_associated_token_account_interface::CreateIdempotentKeys;
use spl_stake_pool_interface::{StakePool, ValidatorList};

use crate::{handle_tx_full, Subcmd};

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

        let authority = authority.map(|a| parse_signer(&a).unwrap());
        let authority = authority
            .as_ref()
            .map_or_else(|| payer.as_ref(), |a| a.as_ref());

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
            .get_multiple_accounts(&[validator_list_pk, authority_ata])
            .await
            .unwrap();

        let maybe_fetched_authority_ata = fetched.pop().unwrap();

        let mut ixs = match maybe_fetched_authority_ata {
            Some(_) => vec![],
            None => {
                if !is_mint_to_authority_ata {
                    panic!("mint_to does not exist and is not authority's ATA");
                } else {
                    vec![
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
                    ]
                }
            }
        };

        let fetched_validator_list = fetched.pop().unwrap().unwrap();

        let ValidatorList { validators, .. } =
            <ValidatorList as borsh::BorshDeserialize>::deserialize(
                &mut fetched_validator_list.data.as_slice(),
            )
            .unwrap();

        if !validators.iter().any(|v| v.vote_account_address == voter) {
            panic!("Validator not part of stake pool");
        }

        let (stake_pool_withdraw_auth, _bump) =
            FindWithdrawAuthority { pool }.run_for_prog(&program_id);

        ixs.extend([
            stake::instruction::authorize(
                &stake_account,
                &authority.pubkey(),
                &stake_pool_withdraw_auth,
                StakeAuthorize::Staker,
                None,
            ),
            stake::instruction::authorize(
                &stake_account,
                &authority.pubkey(),
                &stake_pool_withdraw_auth,
                StakeAuthorize::Withdrawer,
                None,
            ),
            // TODO: deposit stake ix
        ]);

        let mut signers = [payer.as_ref(), authority];
        handle_tx_full(&rpc, args.send_mode, &ixs, &[], &mut signers).await;
    }
}
