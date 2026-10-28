use crate::crypto::PublicKey;
use crate::types::{Block, Transaction, TransactionOutput};

use serde::{Deserialize, Serialize};
use std::io::{self, Cursor, Error as IoError, Read, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// Fetch all UTXOs belonging to a public key.
    FetchUTXOs(PublicKey),

    /// UTXOs belonging to a public key.
    /// The `bool` indicates whether the UTXO is currently marked (e.g. reserved in the mempool).
    UTXOs(Vec<(TransactionOutput, bool)>),

    /// Send a transaction to a node (request).
    SubmitTransaction(Transaction),

    /// Broadcast a new transaction to other nodes.
    NewTransaction(Transaction),

    /// Ask the node to prepare the optimal block template,
    /// with the coinbase transaction paying the specified public key.
    FetchTemplate(PublicKey),

    /// A prepared block template.
    Template(Block),

    /// Ask the node to validate a block template.
    /// This helps avoid mining an invalid block (e.g. if a competing
    /// block has been found or transactions were removed from the mempool).
    ValidateTemplate(Block),

    /// Result of validating a block template.
    /// `true` if the template is valid, `false` otherwise.
    TemplateValidity(bool),

    /// Submit a mined block to a node.
    SubmitTemplate(Block),

    /// Ask a node to report all the other nodes it knows about.
    DiscoverNodes,

    /// Response to `DiscoverNodes`.
    NodeList(Vec<String>),

    /// Ask a node what the highest block height is that it knows about,
    /// for comparison with the local blockchain.
    AskDifference(u32),

    /// Response to `AskDifference`.
    /// Positive means the remote chain is ahead; negative means it's behind.
    Difference(i32),

    /// Ask a node to send the block with the specified height.
    FetchBlock(usize),

    /// Broadcast a new block to other nodes.
    NewBlock(Block),
}

//we are going to use lengthe-prefixed encoding for message
// and we are going to use ciborium (CBOR for serialization)

impl Message {
    pub fn encode(&self) -> Result<Vec<u8>, ciborium::ser::Error<IoError>> {
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes);
        Ok(bytes)
    }

    pub fn decode(data: &[u8]) -> Result<Self, ciborium::de::Error<IoError>> {
        ciborium::from_reader(data)
    }

    pub fn send(&self, stream: &mut impl Write) -> Result<(), ciborium::ser::Error<IoError>> {
        let bytes = self.encode()?; // returns ciborium::ser::Error

        let len = bytes.len() as u64;

        stream.write_all(&len.to_be_bytes())?;
        stream.write_all(&bytes)?;

        Ok(())
    }

    pub async fn send_async<T>(&self, stream: &mut T) -> Result<(), ciborium::ser::Error<IoError>>
    where
        T: AsyncWrite + Unpin,
    {
        let bytes = self.encode()?; // returns ciborium::ser::Error

        let mut len = bytes.len() as u64;
        let len_bytes = (bytes.len() as u64).to_be_bytes();
        stream.write_all(&len_bytes).await?;
        stream.write_all(&bytes).await?;

        Ok(())
    }

    pub fn receive(stream: &mut impl Read) -> Result<Self, ciborium::de::Error<IoError>> {
        // Read the 8-byte length prefix (big-endian u64)
        let mut len_bytes = [0u8; 8];
        stream.read_exact(&mut len_bytes)?;
        let len = u64::from_be_bytes(len_bytes) as usize;

        // Read the actual message bytes
        let mut data = vec![0u8; len];
        stream.read_exact(&mut data)?;

        // Decode the message from bytes
        Self::decode(&data)
    }

    pub async fn receive_async(
        stream: &mut (impl AsyncRead + Unpin),
    ) -> Result<Self, ciborium::de::Error<IoError>> {
        // Read the 8-byte length prefix (big-endian u64)
        let mut len_bytes = [0u8; 8];
        stream.read_exact(&mut len_bytes).await?;
        let len = u64::from_be_bytes(len_bytes) as usize;

        // Read the actual message bytes
        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;

        // Decode the message from bytes
        Self::decode(&data)
    }
}
