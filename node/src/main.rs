use std::net::SocketAddr;

use sdk_api::node::node_server::{Node, NodeServer};
use sdk_api::node::{
    AddPeerRequest, AddPeerResponse, ValidateTransactionRequest, ValidateTransactionResponse,
};
use sdk_api::tonic;

use plain_state::heed;
use plain_types::sdk_authorization_ed25519_dalek;

use sdk_authorization_ed25519_dalek::verify_authorized_transaction;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use tonic::{transport::Server, Request, Response, Status};

pub struct PlainNode {
    env: plain_state::heed::Env,
    state: plain_state::State,
    archive: plain_archive::Archive,
    net: plain_net::Net,
}

impl PlainNode {
    pub fn new(bind_addr: SocketAddr) -> Result<Self, Error> {
        let env_path = project_root::get_project_root()?.join("target/plain.mdb");
        let _ = std::fs::remove_dir_all(&env_path);
        std::fs::create_dir_all(&env_path)?;
        let env = heed::EnvOpenOptions::new()
            .map_size(10 * 1024 * 1024) // 10MB
            .max_dbs(plain_state::State::NUM_DBS + plain_archive::Archive::NUM_DBS)
            .open(env_path)?;
        let state = plain_state::State::new(&env)?;
        let archive = plain_archive::Archive::new(&env)?;
        let main_host = "127.0.0.1";
        let main_port = 18443;
        let net = plain_net::Net::new(bind_addr, main_host, main_port)?;
        Ok(Self {
            env,
            state,
            archive,
            net,
        })
    }
}

#[tonic::async_trait]
impl Node for PlainNode {
    async fn validate_transaction(
        &self,
        request: Request<ValidateTransactionRequest>,
    ) -> Result<Response<ValidateTransactionResponse>, Status> {
        let request = request.into_inner();
        let transaction: plain_types::AuthorizedTransaction =
            bincode::deserialize(&request.transaction).map_err(Error::from)?;
        verify_authorized_transaction(&transaction).map_err(Error::from)?;
        let rtxn = self.env.read_txn().map_err(Error::from)?;
        let transaction = self
            .state
            .fill_transaction(&rtxn, &transaction.transaction)
            .map_err(Error::from)?;
        self.state
            .validate_filled_transaction(&transaction)
            .map_err(Error::from)?;
        Ok(Response::new(ValidateTransactionResponse {}))
    }

    async fn add_peer(
        &self,
        request: Request<AddPeerRequest>,
    ) -> Result<Response<AddPeerResponse>, Status> {
        let AddPeerRequest { host, port } = request.into_inner();
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(plain_net::Error::from)
            .map_err(Error::from)?;
        let res = self.net.connect(addr).await;
        dbg!(&res);
        res.map_err(Error::from)?;
        Ok(Response::new(AddPeerResponse {}))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    const DEFAULT_RPC_PORT: u16 = 3000;
    const DEFAULT_NET_PORT: u16 = 4000;
    let args = Cli::parse();
    let rpc_port = args.rpcport.unwrap_or(DEFAULT_RPC_PORT);
    let net_port = args.netport.unwrap_or(DEFAULT_NET_PORT);
    let rpc_addr = format!("[::1]:{rpc_port}").parse()?;
    let net_addr = format!("127.0.0.1:{net_port}").parse()?;
    dbg!(rpc_addr);
    dbg!(net_addr);
    let mut node = PlainNode::new(net_addr)?;
    node.net.run()?;

    Server::builder()
        .add_service(NodeServer::new(node))
        .serve(rpc_addr)
        .await?;

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sdk_authorization_ed25519_dalek error")]
    Authorization(#[from] sdk_authorization_ed25519_dalek::Error),
    #[error("heed error")]
    Heed(#[from] heed::Error),
    #[error("state error")]
    State(#[from] plain_state::Error),
    #[error("bincode error")]
    Bincode(#[from] bincode::Error),
    #[error("IO error")]
    Io(#[from] std::io::Error),
    #[error("net error")]
    Net(#[from] plain_net::Error),
    #[error("archive error")]
    Archive(#[from] plain_archive::Error),
}

impl From<Error> for Status {
    fn from(err: Error) -> Self {
        Self::internal(format!("{}", err))
    }
}

#[derive(Debug, Parser)]
#[clap(author, version, about)]
pub struct Cli {
    /// P2P networking port.
    #[arg(short, long)]
    pub netport: Option<u16>,
    /// RPC port.
    #[arg(short, long)]
    pub rpcport: Option<u16>,
}
