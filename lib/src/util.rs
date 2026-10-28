use k256::pkcs8::der::Reader;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Result as IoResult, Write};
use std::path::Path;
// A Merkle root is just a wrapper around a Hash
use crate::sha256::Hash;
use crate::types::Transaction;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct MerkleRoot(pub Hash);

impl MerkleRoot {
    /// Calculate the Merkle root of a block's transactions.
    pub fn calculate(transactions: &[Transaction]) -> MerkleRoot {
        // Start with the leaf hashes (one hash per transaction)
        let mut layer: Vec<Hash> = transactions.iter().map(|tx| Hash::hash(tx)).collect();

        // Decide what to do for empty transaction list
        if layer.is_empty() {
            // You can change this if you want a different convention
            return MerkleRoot(Hash::zero());
        }

        // Build the tree upwards until only one hash remains
        while layer.len() > 1 {
            let mut new_layer: Vec<Hash> = Vec::with_capacity((layer.len() + 1) / 2);

            for pair in layer.chunks(2) {
                let left = pair[0];
                // If there's no right element (odd count), duplicate the left
                let right = *pair.get(1).unwrap_or(&pair[0]);

                // Hash the concatenation (or serialized pair) of left and right
                let parent = Hash::hash(&[left, right]);
                new_layer.push(parent);
            }

            layer = new_layer;
        }

        MerkleRoot(layer[0])
    }
}

pub trait Savable
where
    Self: Sized,
{
    fn load<I: Read>(Reader: I) -> IoResult<Self>;
    fn save<O: Write>(&self, Writer: O) -> IoResult<()>;

    fn save_to_file<P: AsRef<Path>>(&self, path: P) -> IoResult<()> {
        let file = File::create(&path)?;
        self.save(file)
    }

    fn load_from_file<P: AsRef<Path>>(path: P) -> IoResult<Self> {
        let file = File::open(path)?;
        Self::load(file)
    }
}
