use ethers::prelude::*;
use ethers::abi::{Function, Param, ParamType, StateMutability, Token};
use anyhow::Result;
use std::sync::{Arc, OnceLock};
use ethers::providers::{Http, Provider};

/// The main Beaug contract address (deployed via CREATE2 for same address across all chains)
pub const MAIN_BEAUG_ADDRESS: &str = "0xe7deB73d0661aA3732c971Ab3d583CFCa786e0d7";

/// The Beaug contract owner address (receives tips from dispersals)
pub const BEAUG_OWNER_ADDRESS: &str = "0xc2D167fd7CD0dC3E0Bd61C5206295C0560e66e31";

/// Cached parsed main Beaug address (parsed once at first access)
static MAIN_BEAUG_ADDRESS_PARSED: OnceLock<Address> = OnceLock::new();

/// Get the parsed main Beaug address, parsing it once and caching
fn main_beaug_address() -> Address {
    *MAIN_BEAUG_ADDRESS_PARSED.get_or_init(|| {
        MAIN_BEAUG_ADDRESS.parse()
            .expect("MAIN_BEAUG_ADDRESS constant is invalid - this is a programming error")
    })
}

/// Get the Beaug disperse contract address
/// Using CREATE2, this address is the same across all EVM chains
pub fn get_disperse_address(_chain_id: u64) -> Option<Address> {
    Some(main_beaug_address())
}

/// Main Beaug registry contract address (same as disperse - it's one contract)
/// This is where contracts register themselves and where we check registration status
pub fn get_beaug_registry_address(_chain_id: u64) -> Option<Address> {
    Some(main_beaug_address())
}

/// Function selector for beaugDisperse(address[],uint256[])
/// keccak256("beaugDisperse(address[],uint256[])") = 0x84a63544...
pub const BEAUG_DISPERSE_SELECTOR: [u8; 4] = [0x84, 0xa6, 0x35, 0x44];

/// Estimate gas for a disperse transaction
pub async fn estimate_disperse_gas(
    _provider: Arc<Provider<Http>>,
    _disperse_address: Address,
    recipients: Vec<Address>,
    _amounts: Vec<U256>,
    _from: Address,
) -> Result<u64> {
    // Base gas + per recipient overhead
    let base_gas = 100_000u64;
    let per_recipient = 50_000u64;
    Ok(base_gas + (per_recipient * recipients.len() as u64))
}

/// Contract validation result
#[derive(Debug, Clone, PartialEq)]
pub enum ContractValidationStatus {
    /// This IS the main Beaug registry contract for this chain
    MainBeaugRegistry,
    /// Contract is registered with Beaug and has compatible function signature
    RegisteredAndCompatible,
    /// Contract is registered but doesn't have the expected function signature
    RegisteredButIncompatible,
    /// Contract has compatible function signature but is not registered
    CompatibleButUnregistered,
    /// Contract is neither registered nor has compatible signature
    Unknown,
    /// Validation is in progress
    Checking,
    /// Error occurred during validation
    Error(String),
}

impl ContractValidationStatus {
    pub fn display_text(&self) -> &'static str {
        match self {
            ContractValidationStatus::MainBeaugRegistry => "Official Beaug Contract",
            ContractValidationStatus::RegisteredAndCompatible => "Verified Beaug Contract",
            ContractValidationStatus::RegisteredButIncompatible => "Registered (signature mismatch)",
            ContractValidationStatus::CompatibleButUnregistered => "Compatible (unregistered)",
            ContractValidationStatus::Unknown => "Unknown contract",
            ContractValidationStatus::Checking => "Checking...",
            ContractValidationStatus::Error(_) => "Validation error",
        }
    }
    
    pub fn is_safe_to_use(&self) -> bool {
        matches!(
            self,
            ContractValidationStatus::MainBeaugRegistry
                | ContractValidationStatus::RegisteredAndCompatible
                | ContractValidationStatus::CompatibleButUnregistered
        )
    }
}

/// Check if a contract is registered with the main Beaug registry
#[allow(deprecated)]
pub async fn check_contract_registered(
    provider: Arc<Provider<Http>>,
    registry_address: Address,
    contract_address: Address,
) -> Result<bool> {
    // Build isRegistered(address) call
    let func = Function {
        name: "isRegistered".to_string(),
        inputs: vec![Param {
            name: "contractAddr".to_string(),
            kind: ParamType::Address,
            internal_type: None,
        }],
        outputs: vec![Param {
            name: "".to_string(),
            kind: ParamType::Bool,
            internal_type: None,
        }],
        constant: None,
        state_mutability: StateMutability::View,
    };
    
    let calldata = func.encode_input(&[Token::Address(contract_address)])?;
    
    let tx = TransactionRequest::new()
        .to(registry_address)
        .data(calldata);
    
    let result = provider.call(&tx.into(), None).await?;
    
    // Decode bool result
    if result.len() >= 32 {
        Ok(result[31] == 1)
    } else {
        Ok(false)
    }
}

/// Check if a contract has the beaugDisperse function by examining its bytecode
pub async fn check_contract_has_beaug_disperse(
    provider: Arc<Provider<Http>>,
    contract_address: Address,
) -> Result<bool> {
    let code = provider.get_code(contract_address, None).await?;
    
    if code.is_empty() {
        return Ok(false);
    }
    
    // Search for the function selector in the bytecode
    // The selector appears in the bytecode as part of the function dispatch
    let selector = &BEAUG_DISPERSE_SELECTOR;
    let code_bytes = code.as_ref();
    
    // Look for the selector in the code (usually in PUSH4 instructions)
    for window in code_bytes.windows(4) {
        if window == selector {
            return Ok(true);
        }
    }
    
    Ok(false)
}

/// Validate a contract address for Beaug compatibility
pub async fn validate_contract(
    provider: Arc<Provider<Http>>,
    chain_id: u64,
    contract_address: Address,
) -> ContractValidationStatus {
    // Check if this IS the main Beaug contract by comparing addresses directly
    // This works even if the contract isn't deployed on the current network yet
    if contract_address == main_beaug_address() {
        return ContractValidationStatus::MainBeaugRegistry;
    }
    
    // For other contracts, check if they have code
    let code = match provider.get_code(contract_address, None).await {
        Ok(code) => code,
        Err(e) => return ContractValidationStatus::Error(format!("Failed to get code: {}", e)),
    };
    
    if code.is_empty() {
        return ContractValidationStatus::Error("Address is not a contract".to_string());
    }
    
    // Check for function signature
    let has_beaug_disperse = {
        let selector = &BEAUG_DISPERSE_SELECTOR;
        let code_bytes = code.as_ref();
        code_bytes.windows(4).any(|w| w == selector)
    };
    
    // Check registration status (if we have a registry for this chain)
    let is_registered = if let Some(registry) = get_beaug_registry_address(chain_id) {
        match check_contract_registered(provider, registry, contract_address).await {
            Ok(registered) => registered,
            Err(_) => false, // Assume not registered if check fails
        }
    } else {
        false // No registry for this chain
    };
    
    match (is_registered, has_beaug_disperse) {
        (true, true) => ContractValidationStatus::RegisteredAndCompatible,
        (true, false) => ContractValidationStatus::RegisteredButIncompatible,
        (false, true) => ContractValidationStatus::CompatibleButUnregistered,
        (false, false) => ContractValidationStatus::Unknown,
    }
}
