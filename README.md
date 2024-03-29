# sanctum-spl-stake-pool-cli

CLI for SPL stake pool program.

## Differences From Upstream

### Incomplete

Currently only a small subset of manager/staker facing commands are done.

### Pool initialization uses created mint

Pool initialization uses an already initialized mint instead of a keypair thats created during the process. This allows for admin retention of token metadata upgrade authority even after pool creation.

### Parameterized Program ID

Stake pool program ID is parameterized to allow for use across different deploys of the spl stake pool program.

### Toml file based

This CLI revolves around syncing stake pool state with a config specified in a toml file. All inputs and outputs are in toml file format.
