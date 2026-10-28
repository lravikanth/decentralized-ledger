use anyhow::Result;
use btc_lib::crypto::PublicKey;
use btc_lib::network::Message;
use btc_lib::types::Block;
use btc_lib::util::Savable;
use clap::Parser;
use std::process::exit;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::{env, usize};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{Duration, interval};

use crate::miner::Miner;
pub mod miner;
fn usage() -> ! {
    println!(
        "Usage {} <address> <Public_key_file>",
        env::args().next().unwrap()
    );
    exit(1);
}
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    address: String,

    #[arg(short, long)]
    public_key_file: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // let address = match env::args().nth(1) {
    //     Some(addr) => addr,
    //     None => usage(),
    // };

    // let public_key_file = match env::args().nth(2) {
    //     Some(pub_key_file) => pub_key_file,
    //     None => usage(),
    // };

    let cli = Cli::parse();
    let public_key = match PublicKey::load_from_file(&cli.public_key_file) {
        Ok(pk) => pk,
        Err(_) => {
            println!("Error extracting public key");
            exit(1);
        }
    };

    // println!("Connecting to {address} to mine with {public_key:?}");

    let mut stream = match TcpStream::connect(&cli.address).await {
        Ok(stream) => stream,
        Err(e) => {
            println!("Failed to connect to server {}", e);
            exit(1)
        }
    };
    println!("Requesting for work from {}", cli.address);
    let miner = Miner::new(cli.address, public_key).await?;
    miner.run().await

    //let Message = Message::FetchTemplate(public_key);
    //Message.send_async(&mut stream);
}
