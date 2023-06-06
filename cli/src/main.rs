use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use plain_types::{
    bitcoin::{self, Amount},
    sdk_types::Address,
    AuthorizedTransaction, Transaction,
};
use sdk_api::{
    node::{node_client::NodeClient, AddPeerRequest, ValidateTransactionRequest},
    tonic::Request,
};

#[tokio::main]
async fn main() -> Result<()> {
    const DEFAULT_RPC_PORT: u32 = 3000;
    let args = Cli::parse();
    let port = args.port.unwrap_or(DEFAULT_RPC_PORT);
    let mut client = NodeClient::connect(format!("http://[::1]:{port}")).await?;

    match args.command {
        Command::Net(net) => match net {
            Net::Connect { host, port } => {
                println!("connect to {host}:{port}");
                let request = Request::new(AddPeerRequest { host, port });
                client.add_peer(request).await?;
            }
        },
    }
    let transaction = AuthorizedTransaction {
        authorizations: vec![],
        transaction: Transaction {
            inputs: vec![],
            outputs: vec![],
        },
    };
    let transaction = bincode::serialize(&transaction)?;
    let request = Request::new(ValidateTransactionRequest { transaction });
    client.validate_transaction(request).await?;
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
    /*
    /// Blind merged mining commands.
    #[command(subcommand)]
    Bmm(Bmm),
    #[command(subcommand)]
    Wallet(Wallet),
    */
}

#[derive(Debug, Subcommand)]
pub enum Wallet {
    Getbalance,
    Getnewaddress,
    Send {
        to: Address,
        #[arg(value_parser = btc_amount_parser)]
        amount: Amount,
        #[arg(value_parser = btc_amount_parser)]
        fee: Amount,
    },
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
