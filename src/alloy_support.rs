use alloy::primitives::Address;
use anyhow::{Context, Result};

pub fn parse_evm_address(address: &str) -> Result<Address> {
    address
        .parse::<Address>()
        .with_context(|| format!("invalid EVM address: {address}"))
}
