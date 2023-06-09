use anyhow::Result;
use clap::{Parser, Subcommand};
use plain_miner::Miner;
use plain_types::{
    bitcoin::{self, hashes::Hash, Amount},
    sdk_types,
    sdk_types::{Address, BlockHash},
    Body, Header,
};
use sdk_api::{
    node::{node_client::NodeClient, *},
    tonic::Request,
};

#[tokio::main]
async fn main() -> Result<()> {
    const DEFAULT_RPC_PORT: u32 = 3000;
    let args = Cli::parse();
    let port = args.port.unwrap_or(DEFAULT_RPC_PORT);
    let mut client = NodeClient::connect(format!("http://[::1]:{port}")).await?;
    let mut miner = Miner::new(0, "localhost", 18443)?;
    let seed = [0; 64];
    let wallet_path = project_root::get_project_root()?.join("target/wallet.mdb");
    let wallet = plain_wallet::Wallet::new(seed, &wallet_path)?;

    match args.command {
        Command::Net(net) => match net {
            Net::Connect { host, port } => {
                println!("connect to {host}:{port}");
                let request = Request::new(AddPeerRequest { host, port });
                client.add_peer(request).await?;
            }
        },
        Command::Wallet(wallet_cmd) => match wallet_cmd {
            Wallet::Newaddress { deposit } => {
                let address = wallet.get_new_address()?;
                let address = match deposit {
                    true => format_deposit_address(&format!("{address}")),
                    false => format!("{address}"),
                };
                println!("{address}");
            }
        },
        Command::Chain(chain) => match chain {
            Chain::Besthash => {
                let best_hash = {
                    let response = client
                        .get_best_hash(Request::new(GetBestHashRequest {}))
                        .await?
                        .into_inner();
                    let hash: [u8; 32] = response.best_hash.try_into().unwrap();
                    BlockHash::from(hash)
                };
                println!("{best_hash}");
            }
            Chain::Height => {
                let block_count = client
                    .get_chain_height(Request::new(GetChainHeightRequest {}))
                    .await?
                    .into_inner()
                    .block_count;
                println!("{block_count}");
            }
        },
        Command::Bmm(bmm) => match bmm {
            Bmm::Attempt { amount } => {
                println!("attempting BMM with {amount}");
                let request = Request::new(GetTransactionsRequest {});
                let transactions = client.get_transactions(request).await?;
                let transactions: Vec<_> = transactions
                    .into_inner()
                    .transactions
                    .iter()
                    .map(Vec::as_slice)
                    .map(bincode::deserialize)
                    .collect::<Result<_, _>>()?;
                let coinbase = vec![];
                let body = Body::new(transactions, coinbase);
                let prev_side_hash = {
                    let response = client
                        .get_best_hash(Request::new(GetBestHashRequest {}))
                        .await?
                        .into_inner();
                    let hash: [u8; 32] = response.best_hash.try_into().unwrap();
                    BlockHash::from(hash)
                };
                let prev_main_hash = miner.drivechain.get_mainchain_tip().await?;
                let header = Header {
                    merkle_root: body.compute_merkle_root(),
                    prev_side_hash,
                    prev_main_hash,
                };
                miner.attempt_bmm(amount.to_sat(), 0, header, body).await?;
                loop {
                    if let Some((header, body)) = miner.confirm_bmm().await.unwrap_or_else(|err| {
                        // dbg!(err);
                        None
                    }) {
                        let header = bincode::serialize(&header)?;
                        let body = bincode::serialize(&body)?;
                        client
                            .submit_block(Request::new(SubmitBlockRequest { header, body }))
                            .await?;
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            Bmm::Confirm => println!("confirming BMM"),
            Bmm::Generate { amount } => println!("creating a block with {amount}"),
        },
    }
    Ok(())
}

#[derive(Debug, Parser)]
#[clap(author, version, about)]
pub struct Cli {
    #[arg(short, long)]
    pub port: Option<u32>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Net {
    Connect {
        #[arg(long)]
        host: String,
        #[arg(short, long)]
        port: u32,
    },
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(subcommand)]
    Net(Net),
    /// Blind merged mining commands.
    #[command(subcommand)]
    Bmm(Bmm),
    /// Block chain commands.
    #[command(subcommand)]
    Chain(Chain),
    #[command(subcommand)]
    Wallet(Wallet),
}

#[derive(Debug, Subcommand)]
pub enum Wallet {
    Newaddress {
        #[arg(short, long, default_value_t = false)]
        deposit: bool,
    },
    /*
    Getbalance,
    Send {
        to: Address,
        #[arg(value_parser = btc_amount_parser)]
        amount: Amount,
        #[arg(value_parser = btc_amount_parser)]
        fee: Amount,
    },
    */
}

#[derive(Debug, Subcommand)]
pub enum Chain {
    /// Get best block hash.
    Besthash,
    /// Get chain height.
    Height,
}

#[derive(Debug, Subcommand)]
pub enum Bmm {
    /// Create a bmm request.
    Attempt {
        /// Amount to be paid to mainchain miners for including the bmm commitment.
        #[arg(value_parser = btc_amount_parser)]
        amount: bitcoin::Amount,
    },
    /// Check if the bmm request was successful, and then connect the block.
    Confirm,
    /// Create a bmm request, generate a mainchain block (only works in regtest mode), confirm bmm.
    Generate {
        /// Amount to be paid to mainchain miners for including the bmm commitment.
        #[arg(value_parser = btc_amount_parser)]
        amount: bitcoin::Amount,
    },
}

fn btc_amount_parser(s: &str) -> Result<bitcoin::Amount, bitcoin::util::amount::ParseAmountError> {
    bitcoin::Amount::from_str_in(s, bitcoin::Denomination::Bitcoin)
}

/// Format `str_dest` with the proper `s{sidechain_number}_` prefix and a
/// checksum postfix for calling createsidechaindeposit on mainchain.
pub fn format_deposit_address(str_dest: &str) -> String {
    let this_sidechain = 0;
    let deposit_address: String = format!("s{}_{}_", this_sidechain, str_dest);
    let hash = sha256::digest(deposit_address.as_bytes()).to_string();
    let hash: String = hash[..6].into();
    format!("{}{}", deposit_address, hash)
}
