use btc_lib::types::Block;
use btc_lib::util::Savable;
use std::fs::File;
use std::path::Path;
use std::process::exit;
use std::{env, fs};

fn main() {
    let path = if let Some(arg) = env::args().nth(1) {
        arg
    } else {
        println!("Block Printer!");
        exit(1)
    };

    match fs::canonicalize(Path::new(&path)) {
        Ok(full_path) => println!("Full path: {}", full_path.display()),
        Err(err) => eprintln!("Error: {}", err),
    }

    if let Ok(file) = File::open(path) {
        let block = Block::load(file).expect("Failed to load block");
        println!("{:#?}", block);
    }
}
