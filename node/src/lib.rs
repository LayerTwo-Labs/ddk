use heed::{RoTxn, RwTxn};
use plain_net::{PeerState, Request, Response};
use plain_types::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    net::SocketAddr,
    path::Path,
    sync::Arc,
};
use tokio::sync::RwLock;

pub mod api;

pub const THIS_SIDECHAIN: u32 = 0;

#[derive(Clone)]
pub struct Node<A, C, S> {
    pub net: plain_net::Net,
    pub state: plain_state::State<A, C>,
    pub custom_state: S,
    pub archive: plain_archive::Archive<A, C>,
    pub mempool: plain_mempool::MemPool<A, C>,
    pub drivechain: plain_drivechain::Drivechain<C>,
    pub env: heed::Env,
}

impl<
        A: Verify<C>
            + GetAddress
            + Clone
            + Debug
            + Sync
            + Send
            + Serialize
            + for<'de> Deserialize<'de>
            + 'static,
        C: Clone
            + Debug
            + Eq
            + Serialize
            + for<'de> Deserialize<'de>
            + Sync
            + Send
            + GetValue
            + 'static,
        S: Clone + State<A, C> + Send + Sync + 'static,
    > Node<A, C, S>
where
    Error: From<<S as State<A, C>>::Error>,
{
    pub fn new(
        datadir: &Path,
        bind_addr: SocketAddr,
        main_host: &str,
        main_port: u32,
    ) -> Result<Self, Error> {
        let env_path = datadir.join("data.mdb");
        // let _ = std::fs::remove_dir_all(&env_path);
        std::fs::create_dir_all(&env_path)?;
        let env = heed::EnvOpenOptions::new()
            .map_size(10 * 1024 * 1024) // 10MB
            .max_dbs(
                plain_state::State::<A, C>::NUM_DBS
                    + S::NUM_DBS
                    + plain_archive::Archive::<A, C>::NUM_DBS
                    + plain_mempool::MemPool::<A, C>::NUM_DBS,
            )
            .open(env_path)?;
        let state = plain_state::State::new(&env)?;
        let archive = plain_archive::Archive::new(&env)?;
        let mempool = plain_mempool::MemPool::new(&env)?;
        let drivechain = plain_drivechain::Drivechain::new(THIS_SIDECHAIN, main_host, main_port)?;
        let net = plain_net::Net::new(bind_addr)?;
        let custom_state = State::new(&env)?;
        Ok(Self {
            net,
            state,
            custom_state,
            archive,
            mempool,
            drivechain,
            env,
        })
    }

    pub fn get_height(&self) -> Result<u32, Error> {
        let txn = self.env.read_txn()?;
        Ok(self.archive.get_height(&txn)?)
    }

    pub fn get_best_hash(&self) -> Result<plain_types::BlockHash, Error> {
        let txn = self.env.read_txn()?;
        Ok(self.archive.get_best_hash(&txn)?)
    }

    pub fn validate_transaction(
        &self,
        txn: &RoTxn,
        transaction: &AuthorizedTransaction<A, C>,
    ) -> Result<u64, Error> {
        let filled_transaction = self.state.fill_transaction(txn, &transaction.transaction)?;
        for (authorization, spent_utxo) in transaction
            .authorizations
            .iter()
            .zip(filled_transaction.spent_utxos.iter())
        {
            if authorization.get_address() != spent_utxo.address {
                return Err(plain_state::Error::WrongPubKeyForAddress.into());
            }
        }
        if A::verify_transaction(transaction).is_err() {
            return Err(plain_state::Error::AuthorizationError.into());
        }
        self.custom_state
            .validate_filled_transaction(txn, &self.state, &filled_transaction)?;
        let fee = self
            .state
            .validate_filled_transaction(&filled_transaction)?;
        Ok(fee)
    }

    pub async fn submit_transaction(
        &self,
        transaction: &AuthorizedTransaction<A, C>,
    ) -> Result<(), Error> {
        {
            let mut txn = self.env.write_txn()?;
            self.validate_transaction(&txn, &transaction)?;
            self.mempool.put(&mut txn, &transaction)?;
            txn.commit()?;
        }
        for peer in self.net.peers.read().await.values() {
            peer.request(&Request::PushTransaction {
                transaction: transaction.clone(),
            })
            .await?;
        }
        Ok(())
    }

    pub fn get_spent_utxos(&self, outpoints: &[OutPoint]) -> Result<Vec<OutPoint>, Error> {
        let txn = self.env.read_txn()?;
        let mut spent = vec![];
        for outpoint in outpoints {
            if self.state.utxos.get(&txn, outpoint)?.is_none() {
                spent.push(*outpoint);
            }
        }
        Ok(spent)
    }

    pub fn get_utxos_by_addresses(
        &self,
        addresses: &HashSet<Address>,
    ) -> Result<HashMap<OutPoint, Output<C>>, Error> {
        let txn = self.env.read_txn()?;
        let utxos = self.state.get_utxos_by_addresses(&txn, addresses)?;
        Ok(utxos)
    }

    pub fn get_transactions(
        &self,
        number: usize,
    ) -> Result<(Vec<AuthorizedTransaction<A, C>>, u64), Error> {
        let mut txn = self.env.write_txn()?;
        let transactions = self.mempool.take(&txn, number)?;
        let mut fee: u64 = 0;
        let mut returned_transactions = vec![];
        let mut spent_utxos = HashSet::new();
        for transaction in &transactions {
            dbg!(&spent_utxos);
            let inputs: HashSet<_> = transaction.transaction.inputs.iter().copied().collect();
            if !spent_utxos.is_disjoint(&inputs) {
                println!("UTXO double spent");
                self.mempool
                    .delete(&mut txn, &transaction.transaction.txid())?;
                continue;
            }
            if self.validate_transaction(&txn, transaction).is_err() {
                self.mempool
                    .delete(&mut txn, &transaction.transaction.txid())?;
                continue;
            }
            let filled_transaction = self
                .state
                .fill_transaction(&txn, &transaction.transaction)?;
            let value_in: u64 = filled_transaction
                .spent_utxos
                .iter()
                .map(GetValue::get_value)
                .sum();
            let value_out: u64 = filled_transaction
                .transaction
                .outputs
                .iter()
                .map(GetValue::get_value)
                .sum();
            fee += value_in - value_out;
            returned_transactions.push(transaction.clone());
            spent_utxos.extend(transaction.transaction.inputs.clone());
        }
        txn.commit()?;
        Ok((returned_transactions, fee))
    }

    pub async fn submit_block(&self, header: &Header, body: &Body<A, C>) -> Result<(), Error> {
        let last_deposit_block_hash = {
            let txn = self.env.read_txn()?;
            self.state.get_last_deposit_block_hash(&txn)?
        };
        let mut bundle = None;
        {
            let two_way_peg_data = self
                .drivechain
                .get_two_way_peg_data(header.prev_main_hash, last_deposit_block_hash)
                .await?;
            let mut txn = self.env.write_txn()?;
            self.state.validate_body(&txn, &body)?;
            self.custom_state.validate_body(&txn, &self.state, &body)?;
            self.state.connect_body(&mut txn, &body)?;
            self.custom_state
                .connect_body(&mut txn, &self.state, &body)?;
            let height = self.archive.get_height(&txn)?;
            self.state
                .connect_two_way_peg_data(&mut txn, &two_way_peg_data, height)?;
            bundle = self.state.get_pending_withdrawal_bundle(&txn)?;
            self.archive.append_header(&mut txn, &header)?;
            self.archive.put_body(&mut txn, &header, &body)?;
            for transaction in &body.transactions {
                self.mempool.delete(&mut txn, &transaction.txid())?;
            }
            txn.commit()?;
        }
        if let Some(bundle) = bundle {
            let _ = self
                .drivechain
                .broadcast_withdrawal_bundle(bundle.transaction)
                .await;
        }
        Ok(())
    }

    pub async fn connect(&self, addr: SocketAddr) -> Result<(), Error> {
        let peer = self.net.connect(addr).await?;
        let peer0 = peer.clone();
        let node0 = self.clone();
        tokio::spawn(async move {
            loop {
                match node0.peer_listen(&peer0).await {
                    Ok(_) => {}
                    Err(err) => {
                        println!("{:?}", err);
                        break;
                    }
                }
            }
        });
        let peer0 = peer.clone();
        let node0 = self.clone();
        tokio::spawn(async move {
            loop {
                match node0.heart_beat_listen(&peer0).await {
                    Ok(_) => {}
                    Err(err) => {
                        println!("{:?}", err);
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    pub async fn heart_beat_listen(&self, peer: &plain_net::Peer) -> Result<(), Error> {
        let message = match peer.connection.read_datagram().await {
            Ok(message) => message,
            Err(err) => {
                self.net
                    .peers
                    .write()
                    .await
                    .remove(&peer.connection.stable_id());
                let addr = peer.connection.stable_id();
                println!("connection {addr} closed");
                return Err(plain_net::Error::from(err).into());
            }
        };
        let state: PeerState = bincode::deserialize(&message)?;
        *peer.state.write().await = Some(state);
        Ok(())
    }

    pub async fn peer_listen(&self, peer: &plain_net::Peer) -> Result<(), Error> {
        let (mut send, mut recv) = peer
            .connection
            .accept_bi()
            .await
            .map_err(plain_net::Error::from)?;
        let data = recv
            .read_to_end(plain_net::READ_LIMIT)
            .await
            .map_err(plain_net::Error::from)?;
        let message: Request<A, C> = bincode::deserialize(&data)?;
        match message {
            Request::GetBlock { height } => {
                let (header, body) = {
                    let txn = self.env.read_txn()?;
                    (
                        self.archive.get_header(&txn, height)?,
                        self.archive.get_body(&txn, height)?,
                    )
                };
                let response = match (header, body) {
                    (Some(header), Some(body)) => Response::Block { header, body },
                    (_, _) => Response::NoBlock,
                };
                let response = bincode::serialize(&response)?;
                send.write_all(&response)
                    .await
                    .map_err(plain_net::Error::from)?;
                send.finish().await.map_err(plain_net::Error::from)?;
            }
            Request::PushTransaction { transaction } => {
                let valid = {
                    let txn = self.env.read_txn()?;
                    self.validate_transaction(&txn, &transaction)
                };
                match valid {
                    Err(err) => {
                        let response = Response::<A, C>::TransactionRejected;
                        let response = bincode::serialize(&response)?;
                        send.write_all(&response)
                            .await
                            .map_err(plain_net::Error::from)?;
                        return Err(err.into());
                    }
                    Ok(_) => {
                        {
                            let mut txn = self.env.write_txn()?;
                            println!("adding transaction to mempool: {:?}", &transaction);
                            self.mempool.put(&mut txn, &transaction)?;
                            txn.commit()?;
                        }
                        for peer0 in self.net.peers.read().await.values() {
                            if peer0.connection.stable_id() == peer.connection.stable_id() {
                                continue;
                            }
                            peer0
                                .request(&Request::<A, C>::PushTransaction {
                                    transaction: transaction.clone(),
                                })
                                .await?;
                        }
                        let response = Response::<A, C>::TransactionAccepted;
                        let response = bincode::serialize(&response)?;
                        send.write_all(&response)
                            .await
                            .map_err(plain_net::Error::from)?;
                        return Ok(());
                    }
                }
            }
        };
        Ok(())
    }

    pub fn run(&mut self) -> Result<(), Error> {
        // Listening to connections.
        let node = self.clone();
        tokio::spawn(async move {
            loop {
                let incoming_conn = node.net.server.accept().await.unwrap();
                let connection = incoming_conn.await.unwrap();
                for peer in node.net.peers.read().await.values() {
                    if peer.connection.remote_address() == connection.remote_address() {
                        println!(
                            "already connected to {} refusing duplicate connection",
                            connection.remote_address()
                        );
                        connection
                            .close(plain_net::quinn::VarInt::from_u32(1), b"already connected");
                    }
                }
                if connection.close_reason().is_some() {
                    continue;
                }
                println!(
                    "[server] connection accepted: addr={} id={}",
                    connection.remote_address(),
                    connection.stable_id(),
                );
                let peer = plain_net::Peer {
                    state: Arc::new(RwLock::new(None)),
                    connection,
                };
                let node0 = node.clone();
                let peer0 = peer.clone();
                tokio::spawn(async move {
                    loop {
                        match node0.peer_listen(&peer0).await {
                            Ok(_) => {}
                            Err(err) => {
                                println!("{:?}", err);
                                break;
                            }
                        }
                    }
                });
                let node0 = node.clone();
                let peer0 = peer.clone();
                tokio::spawn(async move {
                    loop {
                        match node0.heart_beat_listen(&peer0).await {
                            Ok(_) => {}
                            Err(err) => {
                                println!("{:?}", err);
                                break;
                            }
                        }
                    }
                });
                node.net
                    .peers
                    .write()
                    .await
                    .insert(peer.connection.stable_id(), peer);
            }
        });

        // Heart beat.
        let node = self.clone();
        tokio::spawn(async move {
            loop {
                for peer in node.net.peers.read().await.values() {
                    let block_height = {
                        let txn = node.env.read_txn().unwrap();
                        node.archive.get_height(&txn).unwrap()
                    };
                    let state = PeerState { block_height };
                    peer.heart_beat(&state).unwrap();
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });

        // Request missing headers.
        let node = self.clone();
        tokio::spawn(async move {
            loop {
                for peer in node.net.peers.read().await.values() {
                    if let Some(state) = &peer.state.read().await.as_ref() {
                        let height = {
                            let txn = node.env.read_txn().unwrap();
                            node.archive.get_height(&txn).unwrap()
                        };
                        if state.block_height > height {
                            let response = peer
                                .request(&Request::GetBlock { height: height + 1 })
                                .await
                                .unwrap();
                            match response {
                                Response::Block { header, body } => {
                                    println!("got new header {:?}", &header);
                                    node.submit_block(&header, &body).await.unwrap();
                                }
                                Response::NoBlock => {}
                                Response::TransactionAccepted => {}
                                Response::TransactionRejected => {}
                            };
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("heed error")]
    Heed(#[from] heed::Error),
    #[error("address parse error")]
    AddrParse(#[from] std::net::AddrParseError),
    #[error("quinn error")]
    Io(#[from] std::io::Error),
    #[error("net error")]
    Net(#[from] plain_net::Error),
    #[error("archive error")]
    Archive(#[from] plain_archive::Error),
    #[error("drivechain error")]
    Drivechain(#[from] plain_drivechain::Error),
    #[error("mempool error")]
    MemPool(#[from] plain_mempool::Error),
    #[error("state error")]
    State(#[from] plain_state::Error),
    #[error("bincode error")]
    Bincode(#[from] bincode::Error),
}

pub trait State<A, C>: Sized {
    type Error;
    const NUM_DBS: u32;
    fn new(env: &heed::Env) -> Result<Self, Self::Error>;
    fn validate_filled_transaction(
        &self,
        txn: &RoTxn,
        state: &plain_state::State<A, C>,
        transaction: &FilledTransaction<C>,
    ) -> Result<(), Self::Error>;
    fn validate_body(
        &self,
        txn: &RoTxn,
        state: &plain_state::State<A, C>,
        body: &Body<A, C>,
    ) -> Result<(), Self::Error>;
    fn connect_body(
        &self,
        txn: &mut RwTxn,
        state: &plain_state::State<A, C>,
        body: &Body<A, C>,
    ) -> Result<(), Self::Error>;
}

impl<A, C> State<A, C> for () {
    type Error = Error;
    const NUM_DBS: u32 = 0;
    fn new(_env: &heed::Env) -> Result<Self, Self::Error> {
        Ok(())
    }
    fn validate_filled_transaction(
        &self,
        txn: &RoTxn,
        state: &plain_state::State<A, C>,
        transaction: &FilledTransaction<C>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn validate_body(
        &self,
        txn: &RoTxn,
        state: &plain_state::State<A, C>,
        body: &Body<A, C>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn connect_body(
        &self,
        txn: &mut RwTxn,
        state: &plain_state::State<A, C>,
        body: &Body<A, C>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}
