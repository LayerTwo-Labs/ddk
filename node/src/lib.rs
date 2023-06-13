use plain_net::{PeerState, Request, Response};
use plain_types::*;
use std::{net::SocketAddr, sync::Arc, vec};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Node {
    pub net: plain_net::Net,
    pub state: plain_state::State,
    pub archive: plain_archive::Archive,
    pub mempool: plain_mempool::MemPool,
    pub drivechain: plain_drivechain::Drivechain,
    pub env: heed::Env,
}

// 1. Transactions are collected into a block.
// 2. Block hash is computed.
// 3. BMM attempt is made.
// 4. BMM attempt is successful.
// 5. Sidechain block is mined, now it is propagated.

impl Node {
    pub fn new(bind_addr: SocketAddr, main_host: &str, main_port: u32) -> Result<Self, Error> {
        let env_path = project_root::get_project_root()?.join("target/net.mdb");
        let _ = std::fs::remove_dir_all(&env_path);
        std::fs::create_dir_all(&env_path)?;
        let env = heed::EnvOpenOptions::new()
            .map_size(10 * 1024 * 1024) // 10MB
            .max_dbs(
                plain_state::State::NUM_DBS
                    + plain_archive::Archive::NUM_DBS
                    + plain_mempool::MemPool::NUM_DBS,
            )
            .open(env_path)?;
        let state = plain_state::State::new(&env)?;
        let archive = plain_archive::Archive::new(&env)?;
        let mempool = plain_mempool::MemPool::new(&env)?;
        const THIS_SIDECHAIN: u32 = 0;
        let drivechain = plain_drivechain::Drivechain::new(THIS_SIDECHAIN, main_host, main_port)?;
        let net = plain_net::Net::new(bind_addr)?;
        Ok(Self {
            net,
            state,
            archive,
            mempool,
            drivechain,
            env,
        })
    }

    pub fn submit_transaction(&self, transaction: &AuthorizedTransaction) -> Result<(), Error> {
        let mut txn = self.env.write_txn()?;
        self.state.validate_transaction(&txn, &transaction)?;
        self.mempool.put(&mut txn, &transaction)?;
        txn.commit().map_err(Error::from)?;
        Ok(())
    }

    pub async fn submit_block(&self, header: &Header, body: &Body) -> Result<(), Error> {
        let last_deposit_block_hash = {
            let txn = self.env.read_txn()?;
            self.state.get_last_deposit_block_hash(&txn)?
        };
        {
            let two_way_peg_data = self
                .drivechain
                .get_two_way_peg_data(header.prev_main_hash, last_deposit_block_hash)
                .await?;
            let mut txn = self.env.write_txn()?;
            self.state.validate_body(&txn, &body)?;
            self.state.connect_body(&mut txn, &body)?;
            self.state
                .connect_two_way_peg_data(&mut txn, &two_way_peg_data)?;
            self.archive.append_header(&mut txn, &header)?;
            self.archive.put_body(&mut txn, &header, &body)?;
            dbg!(&two_way_peg_data);
            dbg!(self.state.get_utxos(&txn)?);
            txn.commit().map_err(Error::from)?;
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
        let message: Request = bincode::deserialize(&data)?;
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
