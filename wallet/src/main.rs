mod core;
mod tasks;
mod ui;
mod util;
use anyhow::Result;
use btc_lib::types::Transaction;
use clap::{Command, Parser, Subcommand};
use core::{Config, Core, FeeConfig, FeeType, Recipient};
use cursive::views::TextContent;
use kanal::bounded;
use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{self, Duration};
use tracing::{debug, info};

use tasks::{handle_transactions1, ui_task, update_balance, update_utxos1};
use util::{big_mode_btc, generate_dummy_config, setup_panic_hook, setup_tracing};

#[derive(Parser)]
#[command(author,version,about,long_about=None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long, value_name = "FILE", default_value = "wallet_config.toml")]
    pub config: PathBuf,
    #[arg(short, long, value_name = "ADDRESS")]
    node: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    GenerateConfig {
        #[arg(short, long, value_name = "FILE", default_value = "wallet_config.toml")]
        output: PathBuf,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing()?;
    setup_panic_hook();

    info!("Starting wallet application");

    let cli = Cli::parse();

    // Handle subcommand: generate config and exit
    if let Some(Commands::GenerateConfig { output }) = &cli.command {
        debug!("Generating dummy config at: {:?}", output);
        return generate_dummy_config(output);
    }

    info!("Loading config from: {:?}", cli.config);

    // Load core
    let mut core = Core::load(cli.config.clone()).await?;

    // Override default node if provided
    if let Some(node) = cli.node {
        info!("Overriding default node with: {}", node);
        core.config.default_node = node;
    }

    // Create channel for transactions
    let (tx_sender, tx_receiver) = kanal::bounded(10);
    core.tx_sender = tx_sender;

    let core = Arc::new(core);

    info!("Starting background tasks");

    // Shared balance text content
    let balance_content = TextContent::new(big_mode_btc(&core));

    // Run all long-running tasks concurrently; exit when any of them finishes
    tokio::select! {
        _ = ui_task(core.clone(), balance_content.clone()) => {
            info!("UI task finished");
        }
        _ = update_balance(core.clone(), balance_content.clone()) => {
            info!("Balance update task finished");
        }
        _ = update_utxos(core.clone()) => {
            info!("UTXO update task finished");
        }
        _ = handle_transactions(tx_receiver.clone_async(), core.clone()) => {
            info!("Transaction handler task finished");
        }
    }

    info!("Application shutting down");
    Ok(())
}

async fn update_utxos(core: Arc<Core>) {
    let mut interval = time::interval(Duration::from_secs(20));
    loop {
        interval.tick().await;
        if let Err(e) = core.fetch_utxos().await {
            eprintln!(" Failed to update UTXOS: {}", e);
        }
    }
}

async fn handle_transactions(rx: kanal::AsyncReceiver<Transaction>, core: Arc<Core>) {
    while let Ok(trasaction) = rx.recv().await {
        if let Err(e) = core.send_transaction(trasaction).await {
            println!("Failed to send transaction {}", e);
        }
    }
}

async fn run_cli(core: Arc<Core>) -> Result<()> {
    loop {
        // Prompt
        print!("> ");
        io::stdout().flush()?;

        // Read a line from stdin
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        // Split into whitespace-separated parts
        let parts: Vec<&str> = input.trim().split_whitespace().collect();

        // If the user just hit Enter, start next loop iteration
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "balance" => {
                let balance = core.get_balance();
                println!("Current balance: {} satoshis", balance);
            }

            "send" => {
                // Expect: send <recipient> <amount>
                if parts.len() != 3 {
                    println!("Usage: send <recipient> <amount>");
                    continue;
                }

                let recipient_name = parts[1];

                // Parse amount as u64
                let amount: u64 = parts[2].parse()?;

                // Find recipient in config
                let recipient_key = core
                    .config
                    .contacts
                    .iter()
                    .find(|r| r.name == recipient_name)
                    .ok_or_else(|| anyhow::anyhow!("Recipient not found"))?
                    .load()? // Recipient -> LoadedRecipient
                    .key; // take the PublicKey

                // Refresh UTXOs before building tx
                if let Err(e) = core.fetch_utxos().await {
                    println!("Failed to fetch UTXOs: {e}");
                    continue;
                }

                // Build transaction
                let transaction = core.create_transaction(&recipient_key, amount).await?;

                // Send transaction through the async channel
                core.tx_sender.send(transaction)?;

                println!("Transaction sent successfully");

                // Refresh UTXOs after sending
                core.fetch_utxos().await?;
            }

            "exit" => break,

            _ => {
                println!("Unknown command");
            }
        }
    }

    Ok(())
}
