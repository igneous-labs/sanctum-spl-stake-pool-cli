use sanctum_solana_test_utils::ExtendedProgramTest;
use solana_program_test::ProgramTest;

pub fn add_spl_stake_pool_prog(pt: ProgramTest) -> ProgramTest {
    pt.add_test_fixtures_account("spl-stake-pool-prog.json")
        .add_test_fixtures_account("spl-stake-pool-prog-data.json")
}
