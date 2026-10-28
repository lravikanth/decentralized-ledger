use anyhow::{Ok, Result, anyhow};
use btc_lib::crypto::PublicKey;
use btc_lib::network::Message;
use btc_lib::types::Block;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::net::TcpStream;

use tokio::time::interval;

pub struct Miner {
    address: String,
    public_key: PublicKey,
    stream: tokio::sync::Mutex<TcpStream>,
    current_template: Arc<std::sync::Mutex<Option<Block>>>,
    mining: Arc<AtomicBool>,
    mined_block_sender: flume::Sender<Block>,
    mined_block_receiver: flume::Receiver<Block>,
}

impl Miner {
    pub async fn new(address: String, public_key: PublicKey) -> Result<Self> {
        let stream = TcpStream::connect(&address).await?;
        let (mined_block_sender, mined_block_receiver) = flume::unbounded();
        Ok(Self {
            address,
            public_key,
            stream: tokio::sync::Mutex::new(stream),
            current_template: Arc::new(Mutex::new(None)),
            mining: Arc::new(AtomicBool::new(false)),
            mined_block_sender,
            mined_block_receiver,
        })
    }

    pub async fn run(&self) -> Result<()> {
        // Start the CPU-bound mining worker thread
        self.spawn_mining_thread();

        // Timer that fires every 5 seconds
        let mut template_interval = interval(Duration::from_secs(5));

        loop {
            // Needed because flume receivers are moved into select!
            let receiver = self.mined_block_receiver.clone();

            tokio::select! {
                // Case 1: Timer ticked -> refresh the block template
                _ = template_interval.tick() => {
                    self.fetch_and_validate_template().await?;
                }

                // Case 2: Mining thread produced a block
                std::result::Result::Ok(mined_block) = receiver.recv_async() => {
                    self.submit_block(mined_block).await?;
                }
            }
        }
    }
    pub fn spawn_mining_thread(&self) -> thread::JoinHandle<()> {
        // Clone shared state so it can be moved into the thread
        let template = Arc::clone(&self.current_template);
        let mining = Arc::clone(&self.mining);
        let sender = self.mined_block_sender.clone();

        thread::spawn(move || {
            loop {
                // Only mine if the mining flag is set
                if mining.load(Ordering::Relaxed) {
                    // Lock the current template and clone it out
                    if let Some(mut block) = template.lock().unwrap().clone() {
                        println!("Mining block with target: {}", block.header.target);

                        // Try to mine the block with a given max number of attempts
                        if block.header.mine(2_000_000) {
                            println!("Block mined: {}", block.hash());

                            // Send the mined block back to the async side
                            sender.send(block).expect("Failed to send mined block");

                            // Turn off mining until a new template is fetched
                            mining.store(false, Ordering::Relaxed);
                        }
                    }
                }

                // Yield to let other threads run; avoids hogging the CPU completely
                thread::yield_now();
            }
        })
    }

    pub async fn fetch_and_validate_template(&self) -> Result<()> {
        // If we are NOT currently mining, fetch a new template
        if !self.mining.load(Ordering::Relaxed) {
            self.fetch_template().await?;
        }
        // Otherwise, validate the template we're currently mining on
        else {
            self.validate_template().await?;
        }

        Ok(())
    }

    /// Ask the node for a block template.
    pub async fn fetch_template(&self) -> Result<()> {
        println!("Fetching new template");

        // 1) Build the request message
        let message = Message::FetchTemplate(self.public_key.clone());

        // 2) Lock the stream and send the request
        {
            let mut stream_lock = self.stream.lock().await;
            message.send_async(&mut *stream_lock).await?;
            // `stream_lock` is dropped here at end of this scope
        }

        // 3) Lock the stream again and wait for a response
        let mut stream_lock = self.stream.lock().await;
        let response = Message::receive_async(&mut *stream_lock).await?;

        match response {
            Message::Template(template) => {
                // We’re done with the stream now; drop lock early
                drop(stream_lock);

                println!(
                    "Received new template with target: {}",
                    template.header.target
                );

                // Store the new template so the mining thread can use it
                *self.current_template.lock().unwrap() = Some(template);

                // Turn mining on
                self.mining.store(true, Ordering::Relaxed);

                Ok(())
            }
            _ => Err(anyhow!(
                "Unexpected message received when fetching template"
            )),
        }
    }

    /// Ask the node to validate a block template.
    pub async fn validate_template(&self) -> Result<()> {
        // Take a snapshot (clone) of the current template, if any
        if let Some(template) = self.current_template.lock().unwrap().clone() {
            // 1) Build the validation request
            let message = Message::ValidateTemplate(template);

            // 2) Send the validation request over the TCP stream
            {
                let mut stream_lock = self.stream.lock().await;
                message.send_async(&mut *stream_lock).await?;
                // stream_lock is dropped at end of this block → unlocks the stream
            }

            // 3) Wait for the TemplateValidity response
            let mut stream_lock = self.stream.lock().await;
            let response = Message::receive_async(&mut *stream_lock).await?;

            match response {
                Message::TemplateValidity(valid) => {
                    // Done with the stream; unlock early
                    drop(stream_lock);

                    if !valid {
                        println!("Current template is no longer valid");
                        // Tell the miner thread to stop mining this template
                        self.mining.store(false, Ordering::Relaxed);
                    } else {
                        println!("Current template is still valid");
                    }

                    Ok(())
                }
                _ => Err(anyhow!(
                    "Unexpected message received when validating template"
                )),
            }
        } else {
            // No template set; nothing to validate
            Ok(())
        }
    }

    /// Submit a mined block back to the node.
    pub async fn submit_block(&self, block: Block) -> Result<()> {
        println!("Submitting mined block");
        let message = Message::SubmitTemplate(block);
        let mut stream_lock = self.stream.lock().await;
        message.send_async(&mut *stream_lock).await?;
        self.mining.store(false, Ordering::Relaxed);

        Ok(())
    }
}
