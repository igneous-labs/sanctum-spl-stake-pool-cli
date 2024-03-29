use sanctum_solana_test_utils::ExtendedProgramTest;
use solana_program_test::ProgramTest;

pub fn add_vote_accounts(pt: ProgramTest) -> ProgramTest {
    pt.add_test_fixtures_account("shinobi-vote.json")
        .add_test_fixtures_account("zeta-vote.json")
}
