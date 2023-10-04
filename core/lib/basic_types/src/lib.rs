//! The declaration of the most primitive types used in zkSync network.
//!
//! Most of them are just re-exported from the `web3` crate.

#[macro_use]
mod macros;

pub mod network;

use serde::{de, Deserialize, Deserializer, Serialize};
use web3::ethabi::ethereum_types::FromStrRadixErr;

use std::{
    convert::{Infallible, TryFrom, TryInto},
    fmt,
    num::ParseIntError,
    ops::{Add, Deref, DerefMut, Sub},
    str::FromStr,
};

pub use web3::{
    self, ethabi,
    types::{Address, Bytes, Log, TransactionRequest, H128, H160, H2048, H256, U128, U256, U64},
};

/// Account place in the global state tree is uniquely identified by its address.
/// Binary this type is represented by 160 bit big-endian representation of account address.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Hash, Ord, PartialOrd)]
pub struct AccountTreeId {
    address: Address,
}

impl AccountTreeId {
    pub fn new(address: Address) -> Self {
        Self { address }
    }

    pub fn address(&self) -> &Address {
        &self.address
    }

    #[allow(clippy::wrong_self_convention)] // In that case, reference makes more sense.
    pub fn to_fixed_bytes(&self) -> [u8; 20] {
        let mut result = [0u8; 20];
        result.copy_from_slice(&self.address.to_fixed_bytes());
        result
    }

    pub fn from_fixed_bytes(value: [u8; 20]) -> Self {
        let address = Address::from_slice(&value);
        Self { address }
    }
}

impl Default for AccountTreeId {
    fn default() -> Self {
        Self {
            address: Address::zero(),
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<U256> for AccountTreeId {
    fn into(self) -> U256 {
        let mut be_data = [0u8; 32];
        be_data[12..].copy_from_slice(&self.to_fixed_bytes());
        U256::from_big_endian(&be_data)
    }
}

impl TryFrom<U256> for AccountTreeId {
    type Error = Infallible;

    fn try_from(val: U256) -> Result<Self, Infallible> {
        let mut be_data = vec![0; 32];
        val.to_big_endian(&mut be_data);
        Ok(Self::from_fixed_bytes(be_data[12..].try_into().unwrap()))
    }
}

/// ChainId in the ZkSync network.
#[derive(Copy, Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct L2ChainId(pub U256);

impl<'de> Deserialize<'de> for L2ChainId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;

        // Parse the string as a U256
        // try to parse as decimal first
        let u256 = match U256::from_dec_str(&s) {
            Ok(u) => u,
            Err(_) => {
                // try to parse as hex
                s.parse::<U256>().map_err(de::Error::custom)?
            }
        };
        assert!(
            u256 <= L2ChainId::MAX.into(),
            "Too big chain ID. MAX: {}",
            L2ChainId::MAX
        );
        Ok(L2ChainId(u256))
    }
}
impl FromStr for L2ChainId {
    type Err = FromStrRadixErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Parse the string as a U256
        // try to parse as decimal first
        let u256 = match U256::from_dec_str(s) {
            Ok(u) => u,
            Err(_) => {
                // try to parse as hex
                s.parse::<U256>().map_err(FromStrRadixErr::from)?
            }
        };
        assert!(
            u256 <= L2ChainId::MAX.into(),
            "Too big chain ID. MAX: {}",
            L2ChainId::MAX
        );
        Ok(L2ChainId(u256))
    }
}

impl L2ChainId {
    /// The maximum value of the L2 chain ID.
    // 2^53 - 1 is a max safe integer in JS. In ethereum JS libs chain ID should be the safe integer.
    // Next arithmetic operation: subtract 36 and divide by 2 comes from `v` calculation:
    // v = 2*chainId + 36, that should be save integer as well.
    pub const MAX: u64 = ((1 << 53) - 1 - 36) / 2;
}

impl Default for L2ChainId {
    fn default() -> Self {
        Self(U256::from(270))
    }
}

impl From<u64> for L2ChainId {
    fn from(val: u64) -> Self {
        Self(U256::from(val))
    }
}

basic_type!(
    /// zkSync network block sequential index.
    MiniblockNumber,
    u32
);

basic_type!(
    /// zkSync L1 batch sequential index.
    L1BatchNumber,
    u32
);

basic_type!(
    /// Ethereum network block sequential index.
    L1BlockNumber,
    u32
);

basic_type!(
    /// zkSync account nonce.
    Nonce,
    u32
);

basic_type!(
    /// Unique identifier of the priority operation in the zkSync network.
    PriorityOpId,
    u64
);

basic_type!(
    /// ChainId in the Ethereum network.
    L1ChainId,
    u64
);

#[allow(clippy::derivable_impls)]
impl Default for MiniblockNumber {
    fn default() -> Self {
        Self(0)
    }
}

#[allow(clippy::derivable_impls)]
impl Default for L1BatchNumber {
    fn default() -> Self {
        Self(0)
    }
}

#[allow(clippy::derivable_impls)]
impl Default for L1BlockNumber {
    fn default() -> Self {
        Self(0)
    }
}

#[allow(clippy::derivable_impls)]
impl Default for PriorityOpId {
    fn default() -> Self {
        Self(0)
    }
}
