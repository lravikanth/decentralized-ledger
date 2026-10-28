use std::mem;

use btc_lib::network::Message;
use btc_lib::network::Message::*;
use btc_lib::sha256::Hash;
use btc_lib::types::{Block, BlockChain, BlockHeader, Transaction, TransactionOutput};
use btc_lib::util::MerkleRoot;
use chrono::Utc;
use tokio::net::TcpStream;
use uuid::Uuid;

pub async fn handle_connection(mut socket: TcpStream) {
    loop {
        let message = match Message::receive_async(&mut socket).await {
            Ok(message) => message,
            Err(e) => {
                println!("invlaud message from peer {e} , closing the connection");
                return;
            }
        };

        match message {
            UTXOs(_) | Difference(_) | TemplateValidity(_) | NodeList(_) => {
                println!("I am nother minner not a wallet. Goodbye!");
                return;
            }
            FetchBlock(height) => {
                let blockchain = crate::BLOCKCHAIN.read().await;
                let Some(block) = blockchain.blocks().nth(height).cloned() else {
                    return;
                };
                let Message = NewBlock(block);
                message.send_async(&mut socket).await.unwrap();
            }
            DiscoverNodes => {
                let nodes = crate::NODES
                    .iter()
                    .map(|x| x.key().clone())
                    .collect::<Vec<_>>();
                let message = NodeList(nodes);
                message.send_async(&mut socket).await.unwrap();
            }
            AskDifference(height) => {
                let blockchain = crate::BLOCKCHAIN.read().await;
                let count = blockchain.block_height() as i32 - height as i32;
                let message = Difference(count);
                message.send_async(&mut socket).await.unwrap();
            }
            FetchUTXOs(key) => {
                println!("received request to fetch UTXOs");

                let blockchain = crate::BLOCKCHAIN.read().await;

                let utxos: Vec<_> = blockchain
                    .utxos()
                    .iter()
                    .filter(|(_, (_, txout))| txout.pubkey == key)
                    .map(|(_, (marked, txout))| (txout.clone(), *marked))
                    .collect();

                let message = Message::UTXOs(utxos);
                message.send_async(&mut socket).await.unwrap();
            }
            NewBlock(block) => {
                let mut blockchain = crate::BLOCKCHAIN.write().await;
                println!("recevied new block");
                if blockchain.add_block(block).is_err() {
                    println!("Block Rejected");
                }
            }
            NewTransaction(tx) => {
                let mut blockchain = crate::BLOCKCHAIN.write().await;
                println!("recevied new transaction");
                if blockchain.add_to_mempool(tx).is_err() {
                    println!("transaction Rejected closing connection");
                    return;
                }
            }
            ValidateTemplate(block_template) => {
                println!("received template to validate");

                // Read-lock the blockchain so we can inspect the current tip
                let blockchain = crate::BLOCKCHAIN.read().await;

                // Get the hash of the last block (the current tip), or Hash::zero() if the chain is empty
                let tip_hash = blockchain
                    .blocks()
                    .last()
                    .map(|last_block| last_block.hash())
                    .unwrap_or(Hash::zero());

                // A template is valid if it correctly references the current tip as its previous hash
                let status = block_template.header.prev_block_hash == tip_hash;

                let message = Message::TemplateValidity(status);
                message.send_async(&mut socket).await.unwrap();
            }
            SubmitTemplate(block) => {
                println!("Received allegedly mined template");
                let mut blockchain = crate::BLOCKCHAIN.write().await;
                if let Err(e) = blockchain.add_block(block.clone()) {
                    println!("Block rejected: {e} closing connection");
                    return;
                }
                blockchain.rebuild_utxos();
                println!("Block looks good. Broadcasting");
                let nodes = crate::NODES
                    .iter()
                    .map(|x| x.key().clone())
                    .collect::<Vec<_>>();
                for node in nodes {
                    if let Some(mut stream) = crate::NODES.get_mut(&node) {
                        let message = Message::NewBlock(block.clone());

                        if (message.send_async(&mut *stream).await.is_err()) {
                            println!("Failed to send block to node {}", node);
                        }
                    }
                }
            }
            SubmitTransaction(tx) => {
                println!("Received transaction aboout to validate and submit");
                let mut blockchain = crate::BLOCKCHAIN.write().await;
                if let Err(e) = blockchain.add_to_mempool(tx.clone()) {
                    println!("transaction rejected: {e} closing connection");
                    return;
                }
                blockchain.rebuild_utxos();
                println!("Block looks good. Broadcasting");
                let nodes = crate::NODES
                    .iter()
                    .map(|x| x.key().clone())
                    .collect::<Vec<_>>();
                for node in nodes {
                    if let Some(mut stream) = crate::NODES.get_mut(&node) {
                        let message = Message::NewTransaction(tx.clone());

                        if (message.send_async(&mut *stream).await.is_err()) {
                            println!("Failed to send transaction to node {}", node);
                        }
                    }
                }
            }
            Message::FetchTemplate(pubkey) => {
                println!("received request to fetch block template");

                // Read-lock the blockchain
                let blockchain = crate::BLOCKCHAIN.read().await;

                // -----------------------------
                // 1. Build the transaction list
                // -----------------------------
                let mut transactions = Vec::new();

                // 1a. Insert a placeholder coinbase transaction at index 0
                transactions.push(Transaction {
                    inputs: vec![], // coinbase has no normal inputs
                    outputs: vec![TransactionOutput {
                        pubkey,
                        unique_id: Uuid::new_v4(),
                        value: 0, // will be updated later with reward + fees
                    }],
                });

                // 1b. Add transactions from the mempool, up to BLOCK_TRANSACTION_CAP
                transactions.extend(
                    blockchain
                        .mempool()
                        .iter()
                        .take(btc_lib::BLOCK_TRANSACTION_CAP)
                        .map(|(_, tx)| tx.clone()),
                );

                // --------------------------------
                // 2. Compute initial Merkle root
                // --------------------------------
                let merkle_root = MerkleRoot::calculate(&transactions);

                // --------------------------------
                // 3. Build the block header + block
                // --------------------------------
                let prev_block_hash = blockchain
                    .blocks()
                    .last()
                    .map(|last_block| last_block.hash())
                    .unwrap_or(Hash::zero()); // genesis case

                let header = BlockHeader {
                    timestamp: Utc::now(),
                    prev_block_hash,
                    nonce: 0,
                    target: blockchain.target(),
                    merkle_root,
                };

                let mut block = Block::new(header, transactions);

                // --------------------------------
                // 4. Compute miner fees
                // --------------------------------
                let miner_fees = match block.calculate_miner_fees(&blockchain.utxos()) {
                    Ok(fees) => fees,
                    Err(e) => {
                        eprintln!("error calculating miner fees: {e}");
                        return;
                    }
                };

                // --------------------------------
                // 5. Compute block reward + update coinbase
                // --------------------------------
                let reward = blockchain.calculate_block_reward();

                // coinbase is at index 0, first (and only) output
                block.transactions[0].outputs[0].value = reward + miner_fees;

                // --------------------------------
                // 6. Recalculate Merkle root (coinbase changed)
                // --------------------------------
                block.header.merkle_root = MerkleRoot::calculate(&block.transactions);

                // --------------------------------
                // 7. Send the template back to the miner
                // --------------------------------
                let message = Message::Template(block);
                message.send_async(&mut socket).await.unwrap();
            }
            _ => {}
        };
    }
}
