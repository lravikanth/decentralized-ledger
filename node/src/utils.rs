use anyhow::{Context, Ok, Result};
use btc_lib::network::Message;
use btc_lib::types::BlockChain;
use btc_lib::util::Savable;
use tokio::net::TcpStream;
use tokio::time::{self, Duration};

pub async fn load_blockchain(blockchain_file: &str) -> Result<()> {
    println!("blockchain file exists, loading...");

    // 1) Load the blockchain from disk
    let new_blockchain = BlockChain::load_from_file(blockchain_file)?;
    println!("blockchain loaded");

    // 2) Replace the global in-memory blockchain with the one we just loaded
    let mut blockchain = crate::BLOCKCHAIN.write().await;
    *blockchain = new_blockchain;

    // 3) Rebuild the UTXO set from the loaded chain
    println!("rebuilding utxos...");
    blockchain.rebuild_utxos();
    println!("utxos rebuilt");

    // 4) Adjust mining target/difficulty if needed
    println!("checking if target needs to be adjusted...");
    println!("current target: {}", blockchain.target());
    blockchain.try_adjust_target();
    println!("new target: {}", blockchain.target());

    println!("initialization complete");

    Ok(())
}

pub async fn pupulate_connections(nodes: &[String]) -> Result<()> {
    println!("Trying to connect to other nodes..");
    for node in nodes {
        let mut stream = TcpStream::connect(&node).await?;
        let message = Message::DiscoverNodes;
        message.send_async(&mut stream).await;
        let message = Message::receive_async(&mut stream).await?;

        match message {
            Message::NodeList(child_nodes) => {
                println!("Received child nodee from {}", &node);
                for child_node in child_nodes {
                    println!("Adding node {}", &child_node);
                    let tcp_stream = TcpStream::connect(&child_node).await?;
                    crate::NODES.insert(child_node.clone(), tcp_stream);
                }
            }
            _ => {
                println!("Unexpected message from {}", node);
            }
        };
        crate::NODES.insert(node.clone(), stream);
    }

    Ok(())
}

pub async fn find_longest_chain_node() -> Result<(String, u32)> {
    println!("Finding node with the highest blockchain length...");

    // Take a snapshot of all node addresses (keys) from the NODES map.
    let all_nodes: Vec<String> = crate::NODES
        .iter()
        .map(|entry| entry.key().clone())
        .collect();

    if all_nodes.is_empty() {
        anyhow::bail!("No nodes available in NODES");
    }

    let mut longest_name = String::new();
    let mut longest_count = 0;

    // Ask each node for its blockchain length.
    for node in all_nodes {
        println!("Asking {} for blockchain length", node);

        // Open a fresh connection to the node.
        // (This avoids holding a DashMap lock across .await, which does not compile.)
        let mut stream = TcpStream::connect(&node)
            .await
            .with_context(|| format!("Failed to connect to node {}", node))?;

        // Send AskDifference(0) to request its blockchain length.
        let message = Message::AskDifference(0);
        message
            .send_async(&mut stream)
            .await
            .with_context(|| format!("Failed to send AskDifference to {}", node))?;
        println!("Sent AskDifference to {}", node);

        // Wait for the response.
        let response = Message::receive_async(&mut stream)
            .await
            .with_context(|| format!("Failed to receive response from {}", node))?;

        // Handle the response.
        match response {
            Message::Difference(count) => {
                println!("Received Difference({}) from {}", count, node);

                // Track the node with the largest count so far.
                if count > longest_count as i32 {
                    println!("New longest blockchain: {} blocks from {}", count, node);
                    longest_count = count;
                    longest_name = node.clone();
                }
            }
            other => {
                println!("Unexpected message from {}: {:?}", node, other);
            }
        }
    }

    if longest_name.is_empty() {
        anyhow::bail!("No node responded with a Difference message");
    }

    Ok((longest_name, longest_count as u32))
}

pub async fn download_bloackchain(node: &str, count: u32) -> Result<()> {
    let mut stream = crate::NODES.get_mut(node).unwrap();
    for i in 0..count as usize {
        let message = Message::FetchBlock(i);
        match message {
            Message::NewBlock(block) => {
                let mut blockchain = crate::BLOCKCHAIN.write().await;
                blockchain.add_block(block)?;
            }
            _ => {
                println!("Unexcepted Message from {}", node);
            }
        };
    }

    Ok(())
}
/// Periodically cleans old transactions from the mempool.
pub async fn cleanup() {
    // Run every 30 seconds
    let mut interval = time::interval(Duration::from_secs(30));

    loop {
        interval.tick().await;

        println!("Cleaning the mempool from old transactions...");

        let mut blockchain = crate::BLOCKCHAIN.write().await;
        blockchain.cleanup_mempool();
    }
}

/// Periodically saves the blockchain to disk.
pub async fn save(name: String) {
    // Run every 15 seconds
    let mut interval = time::interval(Duration::from_secs(15));

    loop {
        interval.tick().await;

        println!("Saving blockchain to drive...");

        let blockchain = crate::BLOCKCHAIN.read().await;
        // Safe to unwrap because save_to_file should only fail on IO issues
        blockchain.save_to_file(name.clone()).unwrap();
    }
}
