use sdk_api::node::node_server::{Node, NodeServer};
use sdk_api::node::{ValidateTransactionRequest, ValidateTransactionResponse};
use sdk_api::tonic;

use plain_state::{heed, State};
use plain_types::sdk_authorization_ed25519_dalek;

use sdk_authorization_ed25519_dalek::verify_authorized_transaction;

use anyhow::Result;
use tonic::codegen::http::request;
use tonic::{transport::Server, Request, Response, Status};

pub struct PlainNode {
    env: plain_state::heed::Env,
    state: plain_state::State,
}

impl PlainNode {
    pub fn new() -> Result<Self, Error> {
        let env_path = project_root::get_project_root()?.join("target/plain.mdb");
        let _ = std::fs::remove_dir_all(&env_path);
        std::fs::create_dir_all(&env_path)?;
        let env = heed::EnvOpenOptions::new()
            .map_size(10 * 1024 * 1024) // 10MB
            .max_dbs(State::NUM_DBS)
            .open(env_path)?;
        let state = State::new(&env)?;
        Ok(Self { env, state })
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "[::1]:50051".parse()?;
    let node = PlainNode::new()?;

    Server::builder()
        .add_service(NodeServer::new(node))
        .serve(addr)
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
}

impl From<Error> for Status {
    fn from(err: Error) -> Self {
        Self::internal(format!("{}", err))
    }
}
