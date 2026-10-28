use crate::U256;
use crate::error::{BtcError, Result};
use crate::sha256::Hash;
use crate::types::block::Block;
use crate::types::transaction::{Transaction, TransactionOutput};
use crate::util::{MerkleRoot, Savable};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Result as IoResult};
use std::io::{Read, Write};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlockChain {
    utxos: HashMap<Hash, (bool, TransactionOutput)>,
    blocks: Vec<Block>,
    target: U256,
    #[serde(default, skip_serializing)]
    mempool: Vec<(DateTime<Utc>, Transaction)>,
}

impl BlockChain {
    pub fn new() -> Self {
        BlockChain {
            utxos: HashMap::new(),
            blocks: vec![],
            target: crate::MIN_TARGET,
            mempool: vec![],
        }
    }
    // #region Expose Internal Fields
    pub fn utxos(&self) -> &HashMap<Hash, (bool, TransactionOutput)> {
        &self.utxos
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter()
    }

    pub fn target(&self) -> U256 {
        self.target
    }

    pub fn mempool(&self) -> &[(DateTime<Utc>, Transaction)] {
        &self.mempool
    }
    pub fn block_height(&self) -> u64 {
        self.blocks.len() as u64
    }
    // #endregion Expose Internal Fields

    pub fn add_to_mempool(&mut self, transaction: Transaction) -> Result<()> {
        let mut known_inputs = HashSet::new();

        // 1. Basic input checks: must exist in UTXO set and no duplicates
        for input in &transaction.inputs {
            // Input must point to an existing UTXO
            if !self.utxos.contains_key(&input.prev_transaction_output_hash) {
                return Err(BtcError::InvalidTransaction);
            }

            // Prevent double-spending the same UTXO in this transaction
            if known_inputs.contains(&input.prev_transaction_output_hash) {
                return Err(BtcError::InvalidTransaction);
            }

            // Mark this UTXO as used by this transaction
            known_inputs.insert(input.prev_transaction_output_hash);
        }

        for input in &transaction.inputs {
            if let Some((true, _)) = self.utxos.get(&input.prev_transaction_output_hash) {
                let referencing_tx = self.mempool.iter().enumerate().find(|(_, (_, tx))| {
                    tx.outputs
                        .iter()
                        .any(|output| output.hash() == input.prev_transaction_output_hash)
                });
                if let Some((idx, (_, referencing_transaction))) = referencing_tx {
                    for in_input in &referencing_transaction.inputs {
                        self.utxos
                            .entry(in_input.prev_transaction_output_hash)
                            .and_modify(|(in_mempool_use, _)| {
                                *in_mempool_use = false;
                            });
                    }

                    self.mempool.remove(idx);
                } else {
                    self.utxos
                        .entry(input.prev_transaction_output_hash)
                        .and_modify(|(marked, _)| {
                            *marked = false;
                        });
                }
            }
        }

        // 2. Sum all input values
        let all_inputs: u64 = transaction
            .inputs
            .iter()
            .map(|input| {
                self.utxos
                    .get(&input.prev_transaction_output_hash)
                    .expect("BUG: UTXO must exist here")
                    .1
                    .value
            })
            .sum();

        // 3. Sum all output values
        let all_outputs: u64 = transaction.outputs.iter().map(|output| output.value).sum();

        // 4. Inputs must be >= outputs (no creating free coins)
        if all_inputs < all_outputs {
            return Err(BtcError::InvalidTransaction);
        }

        //mark the utxos are used

        for input in &transaction.inputs {
            self.utxos
                .entry(input.prev_transaction_output_hash)
                .and_modify(|(marked, _)| {
                    *marked = true;
                });
        }

        self.mempool.push((Utc::now(), transaction));
        self.mempool.sort_by_key(|(_, transaction)| {
            let all_inputs = transaction
                .inputs
                .iter()
                .map(|input| {
                    self.utxos
                        .get(&input.prev_transaction_output_hash)
                        .expect("Bug")
                        .1
                        .value
                })
                .sum::<u64>();
            let all_outputs: u64 = transaction.outputs.iter().map(|output| output.value).sum();
            let miner_fee = all_inputs - all_outputs;
            miner_fee
        });
        Ok(())
    }

    pub fn cleanup_mempool(&mut self) {
        let now = Utc::now();
        let mut utxo_hashes_to_unmark: Vec<Hash> = Vec::new();

        // Retain only non-expired mempool transactions
        self.mempool.retain(|(timestamp, transaction)| {
            // If this transaction is too old, drop it
            if now - *timestamp > Duration::seconds(crate::MAX_MEMPOOL_TRANSACTION_AGE as i64) {
                // Collect all UTXOs referenced by its inputs so we can unmark them later
                utxo_hashes_to_unmark.extend(
                    transaction
                        .inputs
                        .iter()
                        .map(|input| input.prev_transaction_output_hash),
                );
                false
            } else {
                true
            }
        });

        // Unmark all UTXOs that were used by the expired transactions
        for hash in utxo_hashes_to_unmark {
            self.utxos.entry(hash).and_modify(|(marked, _)| {
                *marked = false;
            });
        }
    }

    pub fn add_block(&mut self, block: Block) -> Result<()> {
        // check if the block is empty
        if self.blocks.is_empty() {
            // if this the first block then check prev_block_---hash is all zeros.
            if block.header.prev_block_hash != Hash::zero() {
                println!("Zero Hash");
                return Err(BtcError::InvalidBlock);
            }
            self.blocks.push(block);
            Ok(())
        } else {
            let last_block = self.blocks.last().unwrap();
            if block.header.prev_block_hash != last_block.hash() {
                println!("Invalid Block");
                return Err(BtcError::InvalidBlock);
            }
            if (!block.header.hash().matches_target(block.header.target)) {
                println!("Doesnt match the target");
                return Err(BtcError::InvalidBlock);
            }

            let cal_merkle_root = MerkleRoot::calculate(&block.transactions);
            if cal_merkle_root != block.header.merkle_root {
                println!("Invalid merkle root");
                return Err(BtcError::InvalidMerkleRoot);
            }

            if block.header.timestamp <= last_block.header.timestamp {
                return Err(BtcError::InvalidBlock);
            }
            block.verify_transactions(self.block_height(), &self.utxos)?;
            //Remove the transactions from memPool that are now inthe block.
            let block_transactions: HashSet<_> =
                block.transactions.iter().map(|tx| tx.hash()).collect();
            self.mempool
                .retain(|(_, tx)| !block_transactions.contains(&tx.hash()));

            self.blocks.push(block);
            Ok(())
        }
    }

    // Try to adjust the target (difficulty) of the blockchain
    pub fn try_adjust_target(&mut self) {
        // If there are no blocks, nothing to adjust
        if self.blocks.is_empty() {
            return;
        }

        // Only adjust difficulty every DIFFICULTY_UPDATE_INTERVAL blocks
        if self.blocks.len() % crate::DIFFICULTY_UPDATE_INTERVAL as usize != 0 {
            return;
        }

        // Measure the time it took to mine the last DIFFICULTY_UPDATE_INTERVAL blocks

        // Oldest block in the adjustment window
        let start_time = self.blocks
            [self.blocks.len() - crate::DIFFICULTY_UPDATE_INTERVAL as usize]
            .header
            .timestamp;

        // Most recent block
        let end_time = self.blocks.last().unwrap().header.timestamp;

        // Time difference between these two
        let time_diff = end_time - start_time;

        // Convert time_diff (chrono Duration) to seconds
        let time_diff_seconds = time_diff.num_seconds();

        // Ideal number of seconds for this many blocks
        let target_seconds = crate::IDEAL_BLOCK_TIME * crate::DIFFICULTY_UPDATE_INTERVAL;

        // Scale current target by (actual_time / ideal_time)
        let new_target = BigDecimal::parse_bytes(self.target.to_string().as_bytes(), 10)
            .expect("BUG: impossible")
            * (BigDecimal::from(time_diff_seconds) / BigDecimal::from(target_seconds));

        let new_target_str = new_target
            .to_string()
            .split('.')
            .next()
            .expect("Bug expected decimal value")
            .to_owned();

        let new_target = U256::from_str_radix(&new_target_str, 10).expect("bug");

        // Clamp new_target to be within [self.target / 4, self.target * 4]
        let new_target = if new_target < self.target / 4 {
            self.target / 4
        } else if new_target > self.target * 4 {
            self.target * 4
        } else {
            new_target
        };

        // Ensure target does not exceed the protocol maximum (MIN_TARGET)
        self.target = new_target.min(crate::MIN_TARGET);
    }

    // Rebuild UTXO set from the blockchain
    pub fn rebuild_utxos(&mut self) {
        for block in &self.blocks {
            for transaction in &block.transactions {
                // 1. Remove all spent outputs
                for input in &transaction.inputs {
                    self.utxos.remove(&input.prev_transaction_output_hash);
                }

                // 2. Add all newly created outputs
                for output in &transaction.outputs {
                    self.utxos
                        .insert(transaction.hash(), (false, output.clone()));
                }
            }
        }
    }

    pub fn calculate_block_reward(&self) -> u64 {
        let block_height = self.block_height();
        let halvings = block_height / crate::HALVING_INTERVAL;
        (crate::INITIAL_REWARD * 10u64.pow(8)) >> halvings
    }
}

impl Default for BlockChain {
    fn default() -> Self {
        Self::new()
    }
}

impl Savable for BlockChain {
    fn load<I: Read>(reader: I) -> IoResult<Self> {
        ciborium::de::from_reader(reader).map_err(|_| {
            IoError::new(
                IoErrorKind::InvalidData,
                "Failed to deserialize Transaction",
            )
        })
    }

    fn save<O: Write>(&self, writer: O) -> IoResult<()> {
        ciborium::ser::into_writer(self, writer)
            .map_err(|_| IoError::new(IoErrorKind::InvalidData, "Failed to serialize Transaction"))
    }
}
