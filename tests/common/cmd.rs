use std::process::Output;

use assert_cmd::Command;
use sanctum_solana_test_utils::{cli::TempCliConfig, ExtendedBanksClient};
use solana_program_test::{BanksClient, BanksClientError, BanksTransactionResultWithMetadata};

pub fn base_cmd(cfg: &TempCliConfig) -> Command {
    let mut cmd = Command::cargo_bin("splsp").unwrap();
    cmd.arg("--config")
        .arg(cfg.config().path())
        .arg("--send-mode")
        .arg("dump-msg");
    cmd
}

// TODO: add simulating txs to ExtendedBanksClient
pub async fn exec_b64_txs(
    cmd: &mut Command,
    bc: &mut BanksClient,
) -> Vec<Result<BanksTransactionResultWithMetadata, BanksClientError>> {
    let Output {
        stdout,
        status,
        stderr,
    } = cmd.output().unwrap();
    assert!(
        status.success(),
        "{}",
        std::str::from_utf8(&stderr).unwrap()
    );
    let stdout = std::str::from_utf8(&stdout).unwrap();
    // run txs in sequence, waiting on result of the prev before exec-ing next
    let mut res = vec![];
    for b64 in stdout.split('\n') {
        if !b64.is_empty() {
            res.push(bc.exec_b64_tx(b64.as_bytes()).await);
        }
    }
    res
}

pub fn assert_all_txs_success_nonempty(
    exec_res: &[Result<BanksTransactionResultWithMetadata, BanksClientError>],
) {
    if exec_res.is_empty() {
        panic!("exec_res is empty");
    }
    for res in exec_res {
        res.as_ref().unwrap().result.as_ref().unwrap();
    }
}
