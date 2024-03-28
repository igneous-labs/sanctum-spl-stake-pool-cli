pub mod srlut {
    sanctum_macros::declare_program_keys!("KtrvWWkPkhSWM9VMqafZhgnTuozQiHzrBDT8oPcMj3T", []);
}

/*
pub async fn fetch_srlut(rpc: &RpcClient) -> AddressLookupTableAccount {
    let srlut = rpc.get_account(&srlut::ID).await.unwrap();
    AddressLookupTableAccount {
        key: srlut::ID,
        addresses: AddressLookupTable::deserialize(&srlut.data)
            .unwrap()
            .addresses
            .into(),
    }
}
 */
