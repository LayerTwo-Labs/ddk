use std::{net::SocketAddr, sync::Arc};

pub struct Node {
    pub net: plain_net::Net,
    pub state: plain_state::State,
    pub archive: plain_archive::Archive,
    pub mempool: plain_mempool::MemPool,
    pub drivechain: plain_drivechain::Drivechain,
    env: heed::Env,
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
        let drivechain = plain_drivechain::Drivechain::new(main_host, main_port)?;
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

    pub fn run(&mut self) -> Result<(), Error> {
        let net = self.net.clone();
        tokio::spawn(async move {
            loop {
                let incoming_conn = net.server.accept().await.unwrap();
                let connection = incoming_conn.await.unwrap();
                println!(
                    "[server] connection accepted: addr={}",
                    connection.remote_address()
                );
                let peer = plain_net::Peer {
                    state: plain_net::PeerState::default(),
                    connection,
                };
                net.peers
                    .write()
                    .await
                    .insert(peer.connection.remote_address(), peer);
            }
        });
        let net = self.net.clone();
        let archive = self.archive.clone();
        let state = self.state.clone();
        let env = self.env.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });

        let net = self.net.clone();
        let archive = self.archive.clone();
        let state = self.state.clone();
        let env = self.env.clone();
        let drivechain = self.drivechain.clone();
        tokio::spawn(async move {
            let host = "localhost";
            let port = 18443;
            // Collect transactions.
            // Construct a block.
            // BMM
            // Send the block out over the network
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
}
