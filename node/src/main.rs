use anyhow::Result as AnyResult;
use argh::FromArgs;
use btc_lib::types::BlockChain;
use dashmap::DashMap;
use static_init::dynamic;
use std::path::Path;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
mod handler;
mod utils;

#[derive(FromArgs)]
///A block chain node
struct Args {
    #[argh(option, default = "9000")]
    ///port number
    port: u16,
    #[argh(option, default = "String::from(\"./blockchain.cbor\")")]
    ///block chain file locatiton
    blockchain_file: String,
    #[argh(positional)]
    ///addresses of initial nodes
    nodes: Vec<String>,
}

#[dynamic]
pub static BLOCKCHAIN: RwLock<BlockChain> = RwLock::new(BlockChain::new());
#[dynamic]
pub static NODES: DashMap<String, TcpStream> = DashMap::new();

#[tokio::main]
async fn main() -> AnyResult<()> {
    let args: Args = argh::from_env();

    let port = args.port;
    let blockchain_file = args.blockchain_file;
    let nodes = args.nodes;

    if Path::new(&blockchain_file).exists() {
        utils::load_blockchain(&blockchain_file).await?;
    } else {
        println!("blockchain file does not exist!");
        utils::pupulate_connections(&nodes);
        println!("total number of nodes: {}", NODES.len());

        if nodes.is_empty() {
            println!("no initial nodes provided, starting as a seed");
        } else {
            let (longest_name, longest_count) = utils::find_longest_chain_node().await?;
            utils::download_bloackchain(&longest_name, longest_count).await?;
            println!("Blockchain downloaded from {}", longest_name);
            //recalculate UTXOs
            {
                let mut blockchain = BLOCKCHAIN.write().await;
                blockchain.rebuild_utxos();
            }
            {
                let mut blockchain = BLOCKCHAIN.write().await;
                blockchain.try_adjust_target();
            }
        }
    }

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    println!("Listening on {}", addr);

    tokio::spawn(utils::cleanup());
    tokio::spawn(utils::save(blockchain_file.clone()));

    loop {
        let (socket, _) = listener.accept().await?;
        tokio::spawn(handler::handle_connection(socket));
    }

    Ok(())
}
