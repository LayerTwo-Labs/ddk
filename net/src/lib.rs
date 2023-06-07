// TODO: Rename Net to Node
// TODO: Rename Node to Rpc

use anyhow::Result;
use quinn::{ClientConfig, Connection, Endpoint, ServerConfig};
use tokio::sync::RwLock;

use std::collections::HashMap;
use std::{net::SocketAddr, sync::Arc};

// State.
// Archive.

// Keep track of peer state
// Exchange metadata
// Bulk download
// Propagation
//
// Initial block download
//
// 1. Download headers
// 2. Download blocks
// 3. Update the state
pub struct Net {
    client: Endpoint,
    server: Endpoint,
    peer_state: Arc<RwLock<PeerState>>,
    peers: Arc<RwLock<HashMap<SocketAddr, Peer>>>,

    pub state: plain_state::State,
    pub archive: plain_archive::Archive,
    pub mempool: plain_mempool::MemPool,
    env: heed::Env,
}

// 1. Transactions are collected into a block.
// 2. Block hash is computed.
// 3. BMM attempt is made.
// 4. BMM attempt is successful.
// 5. Sidechain block is mined, now it is propagated.

pub struct Peer {
    pub state: PeerState,
    pub connection: Connection,
}

#[derive(Clone, Debug)]
pub struct PeerState {
    pub header_height: u32,
    pub block_height: u32,
}

impl Net {
    pub fn new(bind_addr: SocketAddr) -> Result<Self, Error> {
        let (server, _) = make_server_endpoint(bind_addr)?;
        let client = make_client_endpoint("0.0.0.0:0".parse()?)?;
        let peers = Arc::new(RwLock::new(HashMap::new()));

        let env_path = project_root::get_project_root()?.join("target/net.mdb");
        let _ = std::fs::remove_dir_all(&env_path);
        std::fs::create_dir_all(&env_path)?;
        let env = heed::EnvOpenOptions::new()
            .map_size(10 * 1024 * 1024) // 10MB
            .max_dbs(plain_state::State::NUM_DBS + plain_archive::Archive::NUM_DBS)
            .open(env_path)?;
        let state = plain_state::State::new(&env)?;
        let archive = plain_archive::Archive::new(&env)?;
        let mempool = plain_mempool::MemPool::new(&env)?;
        Ok(Self {
            peer_state: Arc::new(RwLock::new(PeerState {
                header_height: 0,
                block_height: 0,
            })),
            server,
            client,
            peers,
            state,
            archive,
            mempool,
            env,
        })
    }

    pub fn run(&mut self) -> Result<(), Error> {
        let endpoint = self.server.clone();
        let peers = self.peers.clone();
        tokio::spawn(async move {
            loop {
                let incoming_conn = endpoint.accept().await.unwrap();
                let connection = incoming_conn.await.unwrap();
                println!(
                    "[server] connection accepted: addr={}",
                    connection.remote_address()
                );
                let peer = Peer {
                    state: PeerState {
                        header_height: 0,
                        block_height: 0,
                    },
                    connection,
                };
                peers
                    .write()
                    .await
                    .insert(peer.connection.remote_address(), peer);
            }
        });
        let peers = self.peers.clone();
        let state = self.peer_state.clone();
        let archive = self.archive.clone();
        let state = self.state.clone();
        let env = self.env.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });

        let peers = self.peers.clone();
        let state = self.peer_state.clone();
        let archive = self.archive.clone();
        let state = self.state.clone();
        let env = self.env.clone();
        tokio::spawn(async move {
            let host = "localhost";
            let port = 18443;
            let drivechain = plain_drivechain::Drivechain::new(host, port);
            // Collect transactions.
            // Construct a block.
            // BMM
            // Send the block out over the network
        });
        Ok(())
    }

    pub async fn connect(&self, addr: SocketAddr) -> Result<(), Error> {
        let connection = self.client.connect(addr, "localhost")?.await?;
        let peer = Peer {
            state: PeerState {
                header_height: 0,
                block_height: 0,
            },
            connection,
        };
        self.peers
            .write()
            .await
            .insert(peer.connection.remote_address(), peer);
        Ok(())
    }
}

#[allow(unused)]
pub fn make_client_endpoint(bind_addr: SocketAddr) -> Result<Endpoint, Error> {
    let client_cfg = configure_client();
    let mut endpoint = Endpoint::client(bind_addr)?;
    endpoint.set_default_client_config(client_cfg);
    Ok(endpoint)
}

/// Constructs a QUIC endpoint configured to listen for incoming connections on a certain address
/// and port.
///
/// ## Returns
///
/// - a stream of incoming QUIC connections
/// - server certificate serialized into DER format
#[allow(unused)]
pub fn make_server_endpoint(bind_addr: SocketAddr) -> Result<(Endpoint, Vec<u8>), Error> {
    let (server_config, server_cert) = configure_server()?;
    let endpoint = Endpoint::server(server_config, bind_addr)?;
    Ok((endpoint, server_cert))
}

/// Returns default server configuration along with its certificate.
fn configure_server() -> Result<(ServerConfig, Vec<u8>), Error> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = cert.serialize_der()?;
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der.clone())];

    let mut server_config = ServerConfig::with_single_cert(cert_chain, priv_key)?;
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_uni_streams(1_u8.into());

    Ok((server_config, cert_der))
}

/// Dummy certificate verifier that treats any certificate as valid.
/// NOTE, such verification is vulnerable to MITM attacks, but convenient for testing.
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

fn configure_client() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    ClientConfig::new(Arc::new(crypto))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("heed error")]
    Heed(#[from] heed::Error),
    #[error("address parse error")]
    AddrParse(#[from] std::net::AddrParseError),
    #[error("quinn error")]
    Io(#[from] std::io::Error),
    #[error("connect error")]
    Connect(#[from] quinn::ConnectError),
    #[error("connection error")]
    Connection(#[from] quinn::ConnectionError),
    #[error("rcgen")]
    RcGen(#[from] rcgen::RcgenError),
    #[error("accept error")]
    AcceptError,
    #[error("quinn rustls error")]
    QuinnRustls(#[from] quinn::crypto::rustls::Error),
    #[error("archive error")]
    Archive(#[from] plain_archive::Error),
    #[error("mempool error")]
    MemPool(#[from] plain_mempool::Error),
    #[error("state error")]
    State(#[from] plain_state::Error),
}
