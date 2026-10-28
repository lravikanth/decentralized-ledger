use btc_lib::crypto::PrivateKey;
use btc_lib::types::{Transaction, TransactionOutput};
use btc_lib::util::Savable;
use std::path::Path;
use std::process::exit;
use std::{env, fs};
use uuid::Uuid;

fn main() {
    let path = if let Some(arg) = env::args().nth(1) {
        arg
    } else {
        println!("Transaction generator no path given!");
        exit(1)
    };

    match fs::canonicalize(Path::new(&path)) {
        Ok(full_path) => println!("Full path: {}", full_path.display()),
        Err(err) => eprintln!("Error: {}", err),
    }

    let private_key = PrivateKey::new();
    let tx = Transaction::new(
        vec![],
        vec![TransactionOutput {
            unique_id: Uuid::new_v4(),
            value: btc_lib::INITIAL_REWARD * 10u64.pow(8),
            pubkey: private_key.public_key(),
        }],
    );

    tx.save_to_file(path).expect("Failed to save transaction");
}
