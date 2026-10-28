use crate::core::Core;
use crate::ui::run_ui;
use crate::util::big_mode_btc;
use btc_lib::types::Transaction;
use cursive::views::TextContent;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{self, sleep, Duration};
use tracing::*;

pub async fn update_utxos1(core: Arc<Core>) -> JoinHandle<()> {
    tokio::spawn(async move {
        // Create a timer that ticks every 20 seconds
        let mut interval = time::interval(Duration::from_secs(20));

        loop {
            // Wait for the next tick
            interval.tick().await;

            // Try updating UTXOs; log error if it fails
            if let Err(e) = core.fetch_utxos().await {
                error!("Failed to update UTXOs: {}", e);
            }
        }
    })
}

pub async fn handle_transactions1(
    rx: kanal::AsyncReceiver<Transaction>,
    core: Arc<Core>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // Continuously listen for transactions from the async channel
        while let Ok(transaction) = rx.recv().await {
            // Try sending the transaction to the node
            if let Err(e) = core.send_transaction(transaction).await {
                error!("Failed to send transaction: {}", e);
            }
        }

        // When rx is closed (all senders dropped), loop ends and task stops.
    })
}
pub async fn update_balance(core: Arc<Core>, balance_content: TextContent) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(500)).await;

            info!("Updating balance string");

            // Update displayed balance in the UI
            balance_content.set_content(big_mode_btc(&core));
        }
    })
}

pub async fn ui_task(core: Arc<Core>, balance_content: TextContent) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        info!("Running UI");

        if let Err(e) = run_ui(core, balance_content) {
            eprintln!("UI ended with error: {}", e);
        }
    })
}
