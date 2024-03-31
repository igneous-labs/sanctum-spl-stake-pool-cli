use assert_cmd::Command;
use sanctum_solana_test_utils::{
    banks_rpc_server::BanksRpcServer,
    cli::TempCliConfig,
    token::{tokenkeg::TokenkegProgramTest, MockMintArgs},
    ExtendedProgramTest,
};
use solana_program_test::{BanksClient, ProgramTest, ProgramTestContext};
use solana_sdk::{
    clock::Clock, feature_set::stake_raise_minimum_delegation_to_1_sol, hash::Hash, pubkey::Pubkey,
    signature::Keypair, signer::Signer,
};

use crate::common::{add_spl_stake_pool_prog, base_cmd};

use super::{
    add_vote_accounts, SPL_STAKE_POOL_LAST_UPGRADE_EPOCH, SPL_STAKE_POOL_LAST_UPGRADE_SLOT,
};

pub async fn setup_init_manager_payer(
    pt: ProgramTest,
    mint: Pubkey,
    manager: &Keypair,
) -> (Command, TempCliConfig, BanksClient, Hash) {
    let pt = add_spl_stake_pool_prog(pt);
    let pt = add_vote_accounts(pt);
    let pt = pt
        .add_system_account(manager.pubkey(), 1_000_000_000_000)
        .add_tokenkeg_mint_from_args(
            mint,
            MockMintArgs {
                mint_authority: Some(manager.pubkey()),
                freeze_authority: None,
                supply: 0,
                decimals: 9,
            },
        )
        .add_test_fixtures_account("srlut.json");
    setup(pt, manager).await
}

pub async fn setup(
    mut pt: ProgramTest,
    payer: &Keypair,
) -> (Command, TempCliConfig, BanksClient, Hash) {
    // TODO: reenable this cursed feature when it actly goes live
    pt.deactivate_feature(stake_raise_minimum_delegation_to_1_sol::ID);
    let ctx = pt.start_with_context().await;
    ctx.set_sysvar(&Clock {
        slot: SPL_STAKE_POOL_LAST_UPGRADE_SLOT + 1,
        epoch: SPL_STAKE_POOL_LAST_UPGRADE_EPOCH,
        // TODO: these 3 fields might need to be set too
        epoch_start_timestamp: Default::default(),
        leader_schedule_epoch: Default::default(),
        unix_timestamp: Default::default(),
    });

    let ProgramTestContext {
        banks_client,
        last_blockhash,
        payer: _rng_payer,
        ..
    } = ctx;

    let (port, _jh) = BanksRpcServer::spawn_random_unused(banks_client.clone()).await;
    let cfg = TempCliConfig::from_keypair_and_local_port(payer, port);
    let cmd = base_cmd(&cfg);
    (cmd, cfg, banks_client, last_blockhash)
}
