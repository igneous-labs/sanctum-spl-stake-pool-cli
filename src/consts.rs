use spl_stake_pool_interface::Fee;

pub mod srlut {
    sanctum_macros::declare_program_keys!("KtrvWWkPkhSWM9VMqafZhgnTuozQiHzrBDT8oPcMj3T", []);
}

pub const ZERO_FEE: Fee = Fee {
    denominator: 1,
    numerator: 0,
};
