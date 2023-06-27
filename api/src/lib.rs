use node::node_server::Node;
use node::*;
use plain_types::{Address, Body, GetAddress, GetValue, OutPoint, Verify};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Debug;
use std::net::SocketAddr;
use tonic::{Request, Response, Status};

pub use tonic;
pub mod node {
    tonic::include_proto!("node");
}

pub struct PlainApi<A, C, S> {
    node: plain_node::Node<A, C, S>,
}

impl<A, C, S> PlainApi<A, C, S> {
    pub fn new(node: plain_node::Node<A, C, S>) -> Self {
        Self { node }
    }
}

#[tonic::async_trait]
impl<
        A: Verify<C>
            + GetAddress
            + Clone
            + Debug
            + Eq
            + Serialize
            + for<'de> Deserialize<'de>
            + Send
            + Sync
            + 'static,
        C: GetValue
            + Clone
            + Debug
            + Eq
            + Serialize
            + for<'de> Deserialize<'de>
            + Send
            + Sync
            + 'static,
        S: Clone + plain_node::State<A, C> + Send + Sync + 'static,
    > Node for PlainApi<A, C, S>
where
    plain_node::Error: From<<S as plain_node::State<A, C>>::Error>,
{
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
        let transaction =
            bincode::deserialize(&request.into_inner().transaction).map_err(Error::from)?;
        self.node
            .submit_transaction(&transaction)
            .await
            .map_err(Error::from)?;
        return Ok(Response::new(SubmitTransactionResponse {}));
    }

    async fn get_transactions(
        &self,
        _request: Request<GetTransactionsRequest>,
    ) -> Result<Response<GetTransactionsResponse>, Status> {
        const NUM_TRANSACTIONS: usize = 100;
        let (transactions, fee) = self
            .node
            .get_transactions(NUM_TRANSACTIONS)
            .map_err(Error::from)?;
        let serialized_transactions = bincode::serialize(&transactions).map_err(Error::from)?;
        return Ok(Response::new(GetTransactionsResponse {
            transactions: serialized_transactions,
            fee,
        }));
    }

    async fn submit_block(
        &self,
        request: Request<SubmitBlockRequest>,
    ) -> Result<Response<SubmitBlockResponse>, Status> {
        let request = request.into_inner();
        let header: plain_types::Header =
            bincode::deserialize(&request.header).map_err(Error::from)?;
        let body: Body<A, C> = bincode::deserialize(&request.body).map_err(Error::from)?;
        self.node
            .submit_block(&header, &body)
            .await
            .map_err(Error::from)?;
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
        let AddPeerRequest { host, port } = request.into_inner();
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(plain_net::Error::from)
            .map_err(Error::from)?;
        self.node.connect(addr).await.map_err(Error::from)?;
        Ok(Response::new(AddPeerResponse {}))
    }

    async fn get_utxos_by_addresses(
        &self,
        request: Request<GetUtxosByAddressesRequest>,
    ) -> Result<Response<GetUtxosByAddressesResponse>, Status> {
        let addresses: HashSet<Address> =
            bincode::deserialize(&request.into_inner().addresses).map_err(Error::from)?;
        let utxos = self
            .node
            .get_utxos_by_addresses(&addresses)
            .map_err(Error::from)?;
        let utxos = bincode::serialize(&utxos).map_err(Error::from)?;
        Ok(Response::new(GetUtxosByAddressesResponse { utxos }))
    }

    async fn get_spent_utxos(
        &self,
        request: Request<GetSpentUtxosRequest>,
    ) -> Result<Response<GetSpentUtxosResponse>, Status> {
        let outpoints: Vec<OutPoint> =
            bincode::deserialize(&request.into_inner().outpoints).map_err(Error::from)?;
        let spent = self.node.get_spent_utxos(&outpoints).map_err(Error::from)?;
        let spent_outpoints = bincode::serialize(&spent).map_err(Error::from)?;
        Ok(Response::new(GetSpentUtxosResponse { spent_outpoints }))
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
    #[error("net error")]
    Net(#[from] plain_net::Error),
}

impl From<Error> for Status {
    fn from(err: Error) -> Self {
        Self::internal(format!("{:?}", err))
    }
}
