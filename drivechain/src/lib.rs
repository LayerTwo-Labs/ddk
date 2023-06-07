mod client;
use base64::Engine as _;
use bitcoin::hashes::Hash as _;
use bitcoin::util::psbt::serialize::{Deserialize, Serialize};
use client::MainClient;
use jsonrpsee::http_client::{HeaderMap, HttpClient, HttpClientBuilder};
use plain_types::*;
use sdk_types::{bs58, Address, Content, OutPoint};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

const BLOCK_SIZE_LIMIT: usize = 100 * 1024;
const THIS_SIDECHAIN: usize = 0;

#[derive(Clone)]
pub struct Drivechain {
    client: HttpClient,
    block: Arc<Mutex<Option<(Header, Body)>>>,
}

impl Drivechain {
    pub async fn attempt_bmm(
        &self,
        amount: u64,
        height: u32,
        header: Header,
        body: Body,
    ) -> Result<(), Error> {
        let str_hash_prev = header.prev_main_hash.to_string();
        let critical_hash: [u8; 32] = header.hash().into();
        let critical_hash = bitcoin::BlockHash::from_inner(critical_hash);
        let value = self
            .client
            .createbmmcriticaldatatx(
                bitcoin::Amount::from_sat(amount).into(),
                height,
                &critical_hash,
                THIS_SIDECHAIN,
                &str_hash_prev[str_hash_prev.len() - 8..],
            )
            .await?;
        let txid = bitcoin::Txid::from_str(value["txid"]["txid"].as_str().unwrap())?;
        assert_eq!(header.merkle_root, body.compute_merkle_root());
        *self.block.lock().await = Some((header, body));
        Ok(())
    }

    pub async fn confirm_bmm(&self) -> Result<Option<(Header, Body)>, Error> {
        let mut block = self.block.lock().await;
        if let Some((header, body)) = block.clone() {
            self.verify_bmm(&header).await?;
            *block = None;
            return Ok(Some((header, body)));
        }
        Ok(None)
    }

    pub async fn verify_bmm(&self, header: &Header) -> Result<(), Error> {
        let prev_main_hash = header.prev_main_hash;
        let block_hash = self
            .client
            .getblock(&prev_main_hash, None)
            .await?
            .nextblockhash
            .ok_or(Error::NoNextBlock { prev_main_hash })?;
        let value = self
            .client
            .verifybmm(&block_hash, &header.hash().into(), THIS_SIDECHAIN)
            .await?;
        Ok(())
    }

    pub async fn get_mainchain_tip(&self) -> Result<bitcoin::BlockHash, Error> {
        Ok(self.client.getbestblockhash().await?)
    }

    pub async fn get_two_way_peg_data(
        &self,
        end: bitcoin::BlockHash,
        start: Option<bitcoin::BlockHash>,
    ) -> Result<TwoWayPegData, Error> {
        let (deposits, deposit_block_hash) = self.get_deposit_outputs(end, start).await?;
        let bundle_statuses = self.get_withdrawal_bundle_statuses().await?;
        let two_way_peg_data = TwoWayPegData {
            deposits,
            deposit_block_hash,
            bundle_statuses,
        };
        Ok(two_way_peg_data)
    }

    pub async fn broadcast_withdrawal_bundle(
        &self,
        transaction: bitcoin::Transaction,
    ) -> Result<(), Error> {
        let rawtx = transaction.serialize();
        let rawtx = hex::encode(&rawtx);
        self.client
            .receivewithdrawalbundle(THIS_SIDECHAIN, &rawtx)
            .await?;
        Ok(())
    }

    async fn get_deposit_outputs(
        &self,
        end: bitcoin::BlockHash,
        start: Option<bitcoin::BlockHash>,
    ) -> Result<(HashMap<OutPoint, Output>, Option<bitcoin::BlockHash>), Error> {
        let deposits = self
            .client
            .listsidechaindepositsbyblock(THIS_SIDECHAIN, Some(end), start)
            .await?;
        let mut last_block_hash = None;
        let mut last_total = 0;
        let mut outputs = HashMap::new();
        dbg!(last_total);
        for deposit in &deposits {
            let transaction = hex::decode(&deposit.txhex)?;
            let transaction = bitcoin::Transaction::deserialize(transaction.as_slice())?;
            if let Some(start) = start {
                if deposit.hashblock == start {
                    last_total = transaction.output[deposit.nburnindex].value;
                    continue;
                }
            }
            let total = transaction.output[deposit.nburnindex].value;
            let value = total - last_total;
            let address: Address = deposit.strdest.parse()?;
            let output = Output {
                address,
                content: Content::Value(value),
            };
            let outpoint = OutPoint::Deposit(bitcoin::OutPoint {
                txid: transaction.txid(),
                vout: deposit.nburnindex as u32,
            });
            outputs.insert(outpoint, output);
            last_total = total;
            last_block_hash = Some(deposit.hashblock);
        }
        Ok((outputs, last_block_hash))
    }

    async fn get_withdrawal_bundle_statuses(
        &self,
    ) -> Result<HashMap<bitcoin::Txid, WithdrawalBundleStatus>, Error> {
        let mut statuses = HashMap::new();
        for spent in &self.client.listspentwithdrawals().await? {
            if spent.nsidechain == THIS_SIDECHAIN {
                statuses.insert(spent.hash, WithdrawalBundleStatus::Confirmed);
            }
        }
        for failed in &self.client.listfailedwithdrawals().await? {
            statuses.insert(failed.hash, WithdrawalBundleStatus::Failed);
        }
        Ok(statuses)
    }

    pub fn new(host: &str, port: u32) -> Result<Self, Error> {
        let mut headers = HeaderMap::new();
        let auth = format!("{}:{}", "user", "password");
        let header_value = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(auth)
        )
        .parse()?;
        headers.insert("authorization", header_value);
        let client = HttpClientBuilder::default()
            .set_headers(headers.clone())
            // http://localhost:18443
            .build(format!("http://{host}:{port}"))?;
        Ok(Drivechain {
            client,
            block: Arc::new(Mutex::new(None)),
        })
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum WithdrawalBundleStatus {
    Failed,
    Confirmed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalBundle {
    pub spent_utxos: HashMap<OutPoint, Output>,
    pub transaction: bitcoin::Transaction,
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TwoWayPegData {
    pub deposits: HashMap<OutPoint, Output>,
    pub deposit_block_hash: Option<bitcoin::BlockHash>,
    pub bundle_statuses: HashMap<bitcoin::Txid, WithdrawalBundleStatus>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DisconnectData {
    pub spent_utxos: HashMap<OutPoint, Output>,
    pub deposits: Vec<OutPoint>,
    pub pending_bundles: Vec<bitcoin::Txid>,
    pub spent_bundles: HashMap<bitcoin::Txid, Vec<OutPoint>>,
    pub spent_withdrawals: HashMap<OutPoint, Output>,
    pub failed_withdrawals: Vec<bitcoin::Txid>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("jsonrpsee error")]
    Jsonrpsee(#[from] jsonrpsee::core::Error),
    #[error("header error")]
    InvalidHeaderValue(#[from] http::header::InvalidHeaderValue),
    #[error("bs58 decode error")]
    Bs58(#[from] bs58::decode::Error),
    #[error("bitcoin consensus encode error")]
    BitcoinConsensusEncode(#[from] bitcoin::consensus::encode::Error),
    #[error("bitcoin hex error")]
    BitcoinHex(#[from] bitcoin::hashes::hex::Error),
    #[error("hex error")]
    Hex(#[from] hex::FromHexError),
    #[error("no next block for prev_main_hash = {prev_main_hash}")]
    NoNextBlock { prev_main_hash: bitcoin::BlockHash },
}
