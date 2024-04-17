use assert_cmd::Command;
use sanctum_solana_test_utils::cli::TempCliConfig;

pub fn base_cmd(cfg: &TempCliConfig) -> Command {
    let mut cmd = Command::cargo_bin("splsp").unwrap();
    cmd.arg("--config")
        .arg(cfg.config().path())
        .arg("--send-mode")
        .arg("dump-msg");
    cmd
}
