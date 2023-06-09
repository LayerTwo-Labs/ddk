use sdk_api::node::node_server::Node;
use sdk_api::node::*;
use sdk_api::tonic;
use sdk_api::tonic::codegen::http::header;
use tonic::{Request, Response, Status};

pub struct PlainApi {
    node: plain_node::Node,
}

impl PlainApi {
    pub fn new(node: plain_node::Node) -> Self {
        Self { node }
    }
}

#[tonic::async_trait]
impl Node for PlainApi {
    async fn get_chain_height(
        &self,
        request: Request<GetChainHeightRequest>,
    ) -> Result<Response<GetChainHeightResponse>, Status> {
        let txn = self.node.env.read_txn().map_err(Error::from)?;
        let block_count = self
            .node
            .archive
            .get_height(&txn)
            .map_err(Error::from)?
            .into();
        return Ok(Response::new(GetChainHeightResponse { block_count }));
    }

    async fn get_best_hash(
        &self,
        request: Request<GetBestHashRequest>,
    ) -> Result<Response<GetBestHashResponse>, Status> {
        let txn = self.node.env.read_txn().map_err(Error::from)?;
        let best_hash = self
            .node
            .archive
            .get_best_hash(&txn)
            .map_err(Error::from)?
            .into();
        return Ok(Response::new(GetBestHashResponse { best_hash }));
    }

    async fn submit_transaction(
        &self,
        request: Request<SubmitTransactionRequest>,
    ) -> Result<Response<SubmitTransactionResponse>, Status> {
        let mut txn = self.node.env.write_txn().map_err(Error::from)?;
        let transaction =
            bincode::deserialize(&request.into_inner().transaction).map_err(Error::from)?;
        self.node
            .state
            .validate_transaction(&txn, &transaction)
            .map_err(Error::from)?;
        self.node
            .mempool
            .put(&mut txn, &transaction)
            .map_err(Error::from)?;
        // txn.commit().map_err(Error::from)?;
        return Ok(Response::new(SubmitTransactionResponse {}));
    }

    async fn get_transactions(
        &self,
        _request: Request<GetTransactionsRequest>,
    ) -> Result<Response<GetTransactionsResponse>, Status> {
        let txn = self.node.env.read_txn().map_err(Error::from)?;
        const TAKE_NUMBER: usize = 100;
        let transactions = self
            .node
            .mempool
            .take(&txn, TAKE_NUMBER)
            .map_err(Error::from)?;
        let mut serialized_transactions = vec![];
        for transaction in &transactions {
            let serrialized_transaction = bincode::serialize(&transaction).map_err(Error::from)?;
            serialized_transactions.push(serrialized_transaction);
        }
        return Ok(Response::new(GetTransactionsResponse {
            transactions: serialized_transactions,
        }));
    }

    async fn submit_block(
        &self,
        request: Request<SubmitBlockRequest>,
    ) -> Result<Response<SubmitBlockResponse>, Status> {
        let request = request.into_inner();
        let header: plain_types::Header =
            bincode::deserialize(&request.header).map_err(Error::from)?;
        let body: plain_types::Body = bincode::deserialize(&request.body).map_err(Error::from)?;
        let mut txn = self.node.env.write_txn().map_err(Error::from)?;
        self.node
            .state
            .validate_body(&txn, &body)
            .map_err(Error::from)?;
        self.node
            .state
            .connect_body(&mut txn, &body)
            .map_err(Error::from)?;
        self.node
            .archive
            .append_header(&mut txn, &header)
            .map_err(Error::from)?;
        self.node
            .archive
            .put_body(&mut txn, &header, &body)
            .map_err(Error::from)?;
        dbg!(header, body);
        txn.commit().map_err(Error::from)?;
        return Ok(Response::new(SubmitBlockResponse {}));
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
    #[error("heed error")]
    Heed(#[from] heed::Error),
    #[error("bincode error")]
    Bincode(#[from] bincode::Error),
    #[error("IO error")]
    Io(#[from] std::io::Error),
    #[error("node error")]
    Node(#[from] plain_node::Error),
    #[error("mempool error")]
    MemPool(#[from] plain_mempool::Error),
    #[error("state error")]
    State(#[from] plain_state::Error),
    #[error("archive error")]
    Archive(#[from] plain_archive::Error),
}

impl From<Error> for Status {
    fn from(err: Error) -> Self {
        Self::internal(format!("{}", err))
    }
}
