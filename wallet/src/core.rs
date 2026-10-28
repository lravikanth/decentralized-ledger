use anyhow::Result;
use btc_lib::crypto::{PrivateKey, PublicKey, Signature};
use btc_lib::network::Message;
use btc_lib::types::{Transaction, TransactionInput, TransactionOutput};
use btc_lib::util::Savable;
use core::str;
use crossbeam_skiplist::SkipMap;
use futures::lock::Mutex;
use futures::stream;
use kanal::AsyncSender;
use kanal::Sender;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::{debug, info};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
pub struct Key {
    pub public: PathBuf,
    pub private: PathBuf,
}

#[derive(Clone)]
struct LoadedKey {
    public: PublicKey,
    private: PrivateKey,
}

// ---------------------------------------------------------
// Recipient: A contact entry with a name and a public key file path
// ---------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct Recipient {
    pub name: String,
    pub key: PathBuf, // path to public key file
}

// ---------------------------------------------------------
// LoadedRecipient: Same as Recipient, but the key is loaded in memory
// ---------------------------------------------------------
#[derive(Clone)]
pub struct LoadedRecipient {
    pub name: String,
    pub key: PublicKey, // actual loaded PublicKey
}

// Convert Recipient → LoadedRecipient by loading the public key from disk
impl Recipient {
    pub fn load(&self) -> Result<LoadedRecipient> {
        let key = PublicKey::load_from_file(&self.key)?;
        Ok(LoadedRecipient {
            name: self.name.clone(),
            key,
        })
    }
}

// ---------------------------------------------------------
// FeeType: How a transaction fee is calculated
// ---------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub enum FeeType {
    Fixed,   // e.g., always 0.0001 BTC
    Percent, // e.g., 1% of the transaction amount
}

// ---------------------------------------------------------
// FeeConfig: Defines your wallet's fee behavior
// ---------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct FeeConfig {
    pub fee_type: FeeType,
    pub value: f64, // amount or percentage depending on fee_type
}

// ---------------------------------------------------------
// Config: Main wallet configuration file
// ---------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub my_keys: Vec<Key>,        // your wallet keypairs
    pub contacts: Vec<Recipient>, // saved contacts
    pub default_node: String,     // default node to connect to
    pub fee_config: FeeConfig,    // fee settings
}
#[derive(Clone)]
struct UtxoStore {
    my_keys: Vec<LoadedKey>,
    utxos: Arc<SkipMap<PublicKey, Vec<(bool, TransactionOutput)>>>,
}

impl UtxoStore {
    fn new() -> Self {
        UtxoStore {
            my_keys: Vec::new(),
            utxos: Arc::new(SkipMap::new()),
        }
    }

    fn add_key(&mut self, key: LoadedKey) {
        self.my_keys.push(key);
    }
}

//#[derive(Clone)]
pub struct Core {
    pub config: Config,
    utxos: UtxoStore,
    pub tx_sender: Sender<Transaction>,
    pub stream: Mutex<TcpStream>,
}
impl Core {
    /// Create a new Core instance
    fn new(config: Config, utxos: UtxoStore, stream: TcpStream) -> Self {
        let (tx_sender, _) = kanal::bounded(10);
        Self {
            config,
            utxos,
            tx_sender: tx_sender,
            stream: Mutex::new(stream),
        }
    }

    /// Load wallet + UTXOs from a config file path
    pub async fn load(config_path: PathBuf) -> Result<Self> {
        // TODO: load config, load UTXO store, initialize Core
        let config: Config = toml::from_str(&fs::read_to_string(&config_path)?)?;
        let mut utxos = UtxoStore::new();
        let stream = TcpStream::connect(&config.default_node).await?;

        //load keys from string
        for key in &config.my_keys {
            let pub_key = PublicKey::load_from_file(&key.public)?;
            let pri_key = PrivateKey::load_from_file(&key.private)?;

            utxos.add_key(LoadedKey {
                public: pub_key,
                private: pri_key,
            });
        }

        Ok(Core::new(config, utxos, stream))
    }

    /// Fetch latest UTXOs from remote node
    pub async fn fetch_utxos(&self) -> Result<()> {
        debug!("Fetching UTXOs from node: {}", self.config.default_node);

        for key in &self.utxos.my_keys {
            // 1. Ask node for UTXOs for this public key
            let message = Message::FetchUTXOs(key.public.clone());

            // Lock the shared stream for this whole request/response
            let mut stream = self.stream.lock().await;

            // Send the request
            message.send_async(&mut *stream).await?;

            // Receive the response
            let response = Message::receive_async(&mut *stream).await?;

            // Handle response
            if let Message::UTXOs(utxos) = response {
                debug!("Received {} UTXOs for key: {:?}", utxos.len(), key.public);

                // Replace the entire UTXO set for this key.
                // Assuming utxos is Vec<(TransactionOutput, bool)>
                self.utxos.utxos.insert(
                    key.public.clone(),
                    utxos
                        .into_iter()
                        .map(|(output, marked)| (marked, output))
                        .collect(),
                );
            } else {
                return Err(anyhow::anyhow!("Unexpected response from node"));
            }
        }

        info!("UTXOs fetched successfully");
        Ok(())
    }

    /// Send a fully constructed transaction to the node
    pub async fn send_transaction(&self, transaction: Transaction) -> Result<()> {
        let mut stream = TcpStream::connect(&self.config.default_node).await?;

        let message = Message::SubmitTransaction(transaction);
        message.send_async(&mut stream).await?;
        Ok(())
    }

    /// Compute wallet balance based on unspent outputs
    pub fn get_balance(&self) -> u64 {
        self.utxos
            .utxos
            .iter()
            .map(|entry| entry.value().iter().map(|utxo| utxo.1.value).sum::<u64>())
            .sum()
    }

    /// Create a transaction to send funds to a recipient public key
    pub async fn create_transaction(
        &self,
        recipient: &PublicKey,
        amount: u64,
    ) -> Result<Transaction> {
        // 1. Compute fee and total amount needed (amount + fee)
        let fee = self.calculate_fee(amount);
        let total_needed = amount + fee;

        let mut inputs = Vec::new();
        let mut input_sum: u64 = 0;

        // 2. Select UTXOs from our store until we cover `total_needed`
        'outer: for entry in self.utxos.utxos.iter() {
            let pubkey = entry.key();
            let utxos = entry.value();

            for (marked, utxo) in utxos.iter() {
                // Skip UTXOs that are already marked/reserved
                if *marked {
                    continue;
                }

                // Find the corresponding private key for this public key
                let key = self
                    .utxos
                    .my_keys
                    .iter()
                    .find(|k| k.public == *pubkey)
                    .expect("No private key found for public key in UTXO store");

                // Add an input spending this UTXO
                let prev_hash = utxo.hash();

                inputs.push(TransactionInput {
                    prev_transaction_output_hash: prev_hash,
                    signature: Signature::sign_output(&prev_hash, &key.private),
                });

                // Increase the total value we are spending
                input_sum += utxo.value;

                // If we now have enough funds, stop selecting more UTXOs
                if input_sum >= total_needed {
                    break 'outer;
                }
            }
        }

        // 3. If we still don't have enough after scanning all UTXOs, fail
        if input_sum < total_needed {
            return Err(anyhow::anyhow!("Insufficient funds"));
        }

        // 4. Create the main output to the recipient
        let mut outputs = vec![TransactionOutput {
            value: amount,
            unique_id: Uuid::new_v4(),
            pubkey: recipient.clone(),
        }];

        // 5. If input_sum > total_needed, create a change output back to us
        if input_sum > total_needed {
            outputs.push(TransactionOutput {
                value: input_sum - total_needed,
                unique_id: Uuid::new_v4(),
                pubkey: self.utxos.my_keys[0].public.clone(), // TODO: use a proper change key
            });
        }

        // 6. Build and return the transaction
        Ok(Transaction::new(inputs, outputs))
    }

    /// Internal: Calculate fee for a transaction based on config
    pub fn calculate_fee(&self, amount: u64) -> u64 {
        match self.config.fee_config.fee_type {
            FeeType::Fixed => self.config.fee_config.value as u64,
            FeeType::Percent => (amount as u64 * self.config.fee_config.value as u64 / 100) as u64,
        }
    }

    pub async fn send_transaction_async(&self, recipient: &str, amount: u64) -> Result<()> {
        info!("Preparing to send {} satoshis to {}", amount, recipient);

        // Look up recipient in contacts and load their public key
        let recipient_key = self
            .config
            .contacts
            .iter()
            .find(|r| r.name == recipient)
            .ok_or_else(|| anyhow::anyhow!("Recipient not found"))?
            .load()? // Recipient -> LoadedRecipient
            .key; // PublicKey

        // Build the transaction (select UTXOs, sign, add change, etc.)
        let transaction = self.create_transaction(&recipient_key, amount).await?;

        debug!("Sending transaction asynchronously via channel");

        // Enqueue the transaction to be sent by the background handler
        self.tx_sender.send(transaction)?;

        Ok(())
    }
}
