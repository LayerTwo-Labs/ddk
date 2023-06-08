use sdk_api::node::node_server::Node;
use sdk_api::node::{
    AddPeerRequest, AddPeerResponse, GetTransactionsRequest, GetTransactionsResponse,
    SubmitBlockRequest, SubmitBlockResponse, ValidateTransactionRequest,
    ValidateTransactionResponse,
};
use sdk_api::tonic;
use tonic::{Request, Response, Status};

pub struct PlainNode {
    node: plain_node::Node,
}

impl PlainNode {
    pub fn new() -> Result<Self, Error> {
        todo!();
    }
}

#[tonic::async_trait]
impl Node for PlainNode {
    async fn get_transactions(
        &self,
        request: Request<GetTransactionsRequest>,
    ) -> Result<Response<GetTransactionsResponse>, Status> {
        todo!();
    }

    async fn submit_block(
        &self,
        request: Request<SubmitBlockRequest>,
    ) -> Result<Response<SubmitBlockResponse>, Status> {
        todo!();
    }

    async fn validate_transaction(
        &self,
        request: Request<ValidateTransactionRequest>,
    ) -> Result<Response<ValidateTransactionResponse>, Status> {
        /*
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
            */
        todo!();
    }

    async fn add_peer(
        &self,
        request: Request<AddPeerRequest>,
    ) -> Result<Response<AddPeerResponse>, Status> {
        /*
        let AddPeerRequest { host, port } = request.into_inner();
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(plain_net::Error::from)
            .map_err(Error::from)?;
        let res = self.net.connect(addr).await;
        dbg!(&res);
        res.map_err(Error::from)?;
        Ok(Response::new(AddPeerResponse {}))
        */
        todo!();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bincode error")]
    Bincode(#[from] bincode::Error),
    #[error("IO error")]
    Io(#[from] std::io::Error),
    #[error("node error")]
    Node(#[from] plain_node::Error),
}

impl From<Error> for Status {
    fn from(err: Error) -> Self {
        Self::internal(format!("{}", err))
    }
}
