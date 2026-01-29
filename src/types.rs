//! Common types shared across modules.

use ethers::prelude::*;

/// Structure to hold address and balance info for Ledger-derived accounts.
#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub index: u32,
    pub address: Address,
    pub balance: U256,
    pub nonce: u64,
    pub derivation_path: String,
}

