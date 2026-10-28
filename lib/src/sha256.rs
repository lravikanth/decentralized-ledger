use core::fmt;
use std::fmt::write;
use std::path::Display;

use crate::U256;
use serde::{Deserialize, Serialize};
use sha256::digest;

#[derive(Debug, Clone, Serialize, Copy, Deserialize, PartialEq, Eq, Hash)]
pub struct Hash(U256);

impl Hash {
    pub(crate) fn hash<T: Serialize>(data: &T) -> Self {
        let mut serialized: Vec<u8> = vec![];

        if let Err(e) = ciborium::into_writer(data, &mut serialized) {
            panic!("Failed to serilase data: {:?}", e);
        };

        let hash = digest(&serialized);
        let hash_bytes = hex::decode(hash).unwrap();

        Hash(U256::from_little_endian(&hash_bytes))
    }

    pub fn matches_target(&self, target: U256) -> bool {
        self.0 <= target
    }

    pub fn zero() -> Self {
        Hash(U256::zero())
    }

    pub fn as_bytes(&self) -> [u8; 32] {
        self.0.to_little_endian()
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{ :x }", self.0)
    }
}
