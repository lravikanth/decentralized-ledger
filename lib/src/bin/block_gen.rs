use std::path::Path;
use std::process::exit;
use std::{env, fs};

use btc_lib::crypto::PrivateKey;
use btc_lib::sha256::Hash;
use btc_lib::types::{Block, BlockHeader, Transaction, TransactionOutput};
use btc_lib::util::{MerkleRoot, Savable};
use chrono::Utc;
use uuid::Uuid;

fn main() {
    let path = if let Some(arg) = env::args().nth(1) {
        arg
    } else {
        println!("Block generator no path given!");
        exit(1)
    };

    match fs::canonicalize(Path::new(&path)) {
        Ok(full_path) => println!("Full path: {}", full_path.display()),
        Err(err) => eprintln!("Error: {}", err),
    }

    let private_key = PrivateKey::new();

    // 2. Build a coinbase transaction output
    let coinbase_output = TransactionOutput {
        unique_id: Uuid::new_v4(),
        value: btc_lib::INITIAL_REWARD * 10u64.pow(8), // e.g. 50 BTC in satoshis
        pubkey: private_key.public_key(),
    };

    // 3. Build the coinbase transaction (no inputs, 1 output)
    let coinbase_tx = Transaction::new(
        vec![],                // no inputs for coinbase
        vec![coinbase_output], // single output
    );

    // 4. Put the transaction into the block's transaction list
    let transactions = vec![coinbase_tx];

    // 5. Calculate the Merkle root from all transactions
    let merkle_root = MerkleRoot::calculate(&transactions);

    // 6. Build the block header
    let header = BlockHeader::new(
        Utc::now(),   // timestamp
        0,            // nonce (start at 0)
        Hash::zero(), // previous block hash (genesis, so zero)
        merkle_root,
        btc_lib::MIN_TARGET,
    );

    // 7. Build the full block
    let block = Block::new(header, transactions);

    // 8. Save block to file
    block.save_to_file(path).expect("Failed to save block");
}
