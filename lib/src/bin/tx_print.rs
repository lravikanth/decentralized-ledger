use btc_lib::types::Transaction;
use btc_lib::util::Savable;
use std::fs::File;
use std::path::Path;
use std::process::exit;
use std::{env, fs};

fn main() {
    let path = if let Some(arg) = env::args().nth(1) {
        arg
    } else {
        println!("Transaction Printer Failed no path!");
        exit(1)
    };

    match fs::canonicalize(Path::new(&path)) {
        Ok(full_path) => println!("Full path: {}", full_path.display()),
        Err(err) => eprintln!("Error: {}", err),
    }

    if let Ok(file) = File::open(path) {
        let tx = Transaction::load(file).expect("Failed to load transaciton");
        println!("{:#?}", tx);
    }
}
