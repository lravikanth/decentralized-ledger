pub mod crypto;
pub mod error;
pub mod network;
pub mod sha256;
pub mod types;
pub mod util;
use serde::{Deserialize, Serialize};
use uint::construct_uint;

construct_uint! {
    #[derive(Serialize,Deserialize)]
    pub struct U256(4);
}

// Initial block reward (Bitcoin starts at 50 BTC)
// Multiply by 10^8 if using satoshis
pub const INITIAL_REWARD: u64 = 50;

// Number of blocks between reward halvings
pub const HALVING_INTERVAL: u64 = 210;

// Ideal target block time (in seconds)
pub const IDEAL_BLOCK_TIME: u64 = 10;

// Minimum (easiest) possible difficulty target
pub const MIN_TARGET: U256 = U256([
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
    0x0000_FFFF_FFFF_FFFF,
]);

// Disfficulty update interval in blocks
pub const DIFFICULTY_UPDATE_INTERVAL: u64 = 50;
pub const MAX_MEMPOOL_TRANSACTION_AGE: u64 = 600;
pub const BLOCK_TRANSACTION_CAP: usize = 20;
