use std::io::Write;

use sanctum_spl_stake_pool_cli::{ConfigRaw, ConfigTomlFile};
use tempfile::NamedTempFile;

pub fn tmp_config_file(cfg: &ConfigRaw) -> NamedTempFile {
    let f = NamedTempFile::new().unwrap();
    let s = toml::to_string_pretty(&ConfigTomlFile { pool: cfg }).unwrap();
    f.as_file().write_all(s.as_bytes()).unwrap();
    f
}
