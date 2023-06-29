use anyhow::Result;
use clap::{Parser, Subcommand};
use plain_api::{
    node::{node_client::NodeClient, *},
    tonic::Request,
};
use plain_miner::Miner;
use plain_types::{
    bitcoin::{self, Amount},
    Address, BlockHash, Body, GetValue, Header, OutPoint,
};
use plain_wallet::Authorization;
use std::{collections::HashMap, path::PathBuf};

type Output = plain_types::Output<()>;
type AuthorizedTransaction = plain_types::AuthorizedTransaction<Authorization, ()>;

#[tokio::main]
async fn main() -> Result<()> {
    const DEFAULT_RPC_PORT: u32 = 3000;
    let args = Cli::parse();
    let port = args.port.unwrap_or(DEFAULT_RPC_PORT);
    let datadir = args
        .datadir
        .unwrap_or(project_root::get_project_root()?.join("target/plain"));
    let mut client = NodeClient::connect(format!("http://[::1]:{port}")).await?;
    let mut miner = Miner::<Authorization, ()>::new(0, "localhost", 18443)?;
    let wallet_path = datadir.join("wallet.mdb");
    let wallet = plain_wallet::Wallet::<()>::new(&wallet_path)?;
    match args.command {
        Command::Net(net) => match net {
            Net::Connect { host, port } => {
                println!("connect to {host}:{port}");
                let request = Request::new(AddPeerRequest { host, port });
                client.add_peer(request).await?;
            }
        },
        Command::Wallet(wallet_cmd) => match wallet_cmd {
            Wallet::Setseed => {
                let seed = rpassword::prompt_password("seed words: ")?;
                let passphrase = rpassword::prompt_password("passphrase: ")?;
                let seed = bip39::Mnemonic::parse(&seed)?.to_seed(passphrase);
                wallet.set_seed(seed)?;
                println!("new seed was set");
            }
            Wallet::Getnewaddress { deposit } => {
                let address = wallet.get_new_address()?;
                let address = match deposit {
                    true => format_deposit_address(&format!("{address}")),
                    false => format!("{address}"),
                };
                println!("{address}");
            }
            Wallet::Getaddresses { deposit } => {
                let addresses = wallet.get_addresses()?;
                for address in &addresses {
                    let address = match deposit {
                        true => format_deposit_address(&format!("{address}")),
                        false => format!("{address}"),
                    };
                    println!("{address}");
                }
            }
            Wallet::Sync => {
                let addresses = wallet.get_addresses()?;
                let addresses = bincode::serialize(&addresses)?;
                let request = Request::new(GetUtxosByAddressesRequest { addresses });
                let utxos = client
                    .get_utxos_by_addresses(request)
                    .await?
                    .into_inner()
                    .utxos;
                let utxos: HashMap<OutPoint, Output> = bincode::deserialize(&utxos)?;

                let outpoints: Vec<_> = wallet.get_utxos()?.into_keys().collect();
                let outpoints = bincode::serialize(&outpoints)?;
                let request = Request::new(GetSpentUtxosRequest { outpoints });
                let spent = client
                    .get_spent_utxos(request)
                    .await?
                    .into_inner()
                    .spent_outpoints;
                let spent: Vec<_> = bincode::deserialize(&spent)?;
                wallet.put_utxos(&utxos)?;
                wallet.delete_utxos(&spent)?;
                println!("{} new utxos added", utxos.len());
                println!("{} spent utxos deleted", spent.len());
            }
            Wallet::Getbalance => {
                let balance = wallet.get_balance()?;
                let balance = bitcoin::Amount::from_sat(balance);
                println!("{balance}");
            }
            Wallet::Getutxos => {
                let utxos = wallet.get_utxos()?;
                for (outpoint, output) in &utxos {
                    println!("outpoint: {outpoint}");
                    println!("address: {}", output.address,);
                    println!("content: {:?}", output.content);
                    println!();
                }
            }
            Wallet::Send { to, value, fee } => {
                let transaction = wallet.create_transaction(to, value.to_sat(), fee.to_sat())?;
                let transaction = wallet.authorize(transaction)?;
                dbg!(&transaction);
                let transaction = bincode::serialize(&transaction)?;
                let request = Request::new(SubmitTransactionRequest { transaction });
                client.submit_transaction(request).await?;
            }
            Wallet::Withdraw {
                to,
                value,
                main_fee,
                fee,
            } => {
                let transaction = wallet.create_withdrawal(
                    to,
                    value.to_sat(),
                    main_fee.to_sat(),
                    fee.to_sat(),
                )?;
                let transaction = wallet.authorize(transaction)?;
                dbg!(&transaction);
                let transaction = bincode::serialize(&transaction)?;
                let request = Request::new(SubmitTransactionRequest { transaction });
                client.submit_transaction(request).await?;
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
            Bmm::Attempt { value } => {
                println!("attempting BMM with {value}");
                let request = Request::new(GetTransactionsRequest {});
                let transactions = client.get_transactions(request).await?.into_inner();
                let fee = transactions.fee;
                let transactions: Vec<AuthorizedTransaction> =
                    bincode::deserialize(&transactions.transactions)?;
                let coinbase = match fee {
                    0 => vec![],
                    _ => vec![Output {
                        address: wallet.get_new_address()?,
                        content: plain_types::Content::Value(fee),
                    }],
                };
                let body = Body::new(transactions, coinbase);
                dbg!(&body);
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
                miner.attempt_bmm(value.to_sat(), 0, header, body).await?;
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
            Bmm::Generate { value } => println!("creating a block with {value}"),
        },
    }
    Ok(())
}

#[derive(Debug, Parser)]
#[clap(author, version, about)]
pub struct Cli {
    #[arg(short, long)]
    pub datadir: Option<PathBuf>,
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
    Setseed,
    Getnewaddress {
        #[arg(short, long, default_value_t = false)]
        deposit: bool,
    },
    Getaddresses {
        #[arg(short, long, default_value_t = false)]
        deposit: bool,
    },
    Sync,
    Getbalance,
    Getutxos,
    Send {
        to: Address,
        #[arg(value_parser = btc_amount_parser)]
        value: Amount,
        #[arg(value_parser = btc_amount_parser)]
        fee: Amount,
    },
    Withdraw {
        to: bitcoin::Address<bitcoin::address::NetworkUnchecked>,
        #[arg(value_parser = btc_amount_parser)]
        value: Amount,
        #[arg(value_parser = btc_amount_parser)]
        fee: Amount,
        #[arg(value_parser = btc_amount_parser)]
        main_fee: Amount,
    },
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
        /// Value to be paid to mainchain miners for including the bmm commitment.
        #[arg(value_parser = btc_amount_parser)]
        value: bitcoin::Amount,
    },
    /// Check if the bmm request was successful, and then connect the block.
    Confirm,
    /// Create a bmm request, generate a mainchain block (only works in regtest mode), confirm bmm.
    Generate {
        /// Value to be paid to mainchain miners for including the bmm commitment.
        #[arg(value_parser = btc_amount_parser)]
        value: bitcoin::Amount,
    },
}

fn btc_amount_parser(s: &str) -> Result<bitcoin::Amount, bitcoin::amount::ParseAmountError> {
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

// Testing seed 0: resist miss peasant neither curve near chef crush chapter patch run best
// Testing seed 1: valve six lady gossip muscle rather dry elephant void catalog elder surprise
