use solana_sdk::pubkey::Pubkey;

pub fn pubkey_opt_display(pubkey_opt: &Option<Pubkey>) -> String {
    pubkey_opt.map_or_else(|| "None".to_owned(), |pk| pk.to_string())
}
