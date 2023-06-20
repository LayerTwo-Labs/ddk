use bitcoin::hashes::Hash as _;
use plain_drivechain::{Drivechain, MainClient as _};
use plain_types::*;
use sdk_types::bitcoin;
use std::str::FromStr as _;

pub struct Miner {
    pub drivechain: Drivechain<()>,
    block: Option<(Header, Body)>,
    sidechain_number: u32,
}

impl Miner {
    pub fn new(sidechain_number: u32, host: &str, port: u32) -> Result<Self, Error> {
        let drivechain = Drivechain::new(sidechain_number, host, port)?;
        Ok(Self {
            drivechain,
            sidechain_number,
            block: None,
        })
    }

    pub async fn attempt_bmm(
        &mut self,
        amount: u64,
        height: u32,
        header: Header,
        body: Body,
    ) -> Result<(), Error> {
        let str_hash_prev = header.prev_main_hash.to_string();
        let critical_hash: [u8; 32] = header.hash().into();
        let critical_hash = bitcoin::BlockHash::from_inner(critical_hash);
        let value = self
            .drivechain
            .client
            .createbmmcriticaldatatx(
                bitcoin::Amount::from_sat(amount).into(),
                height,
                &critical_hash,
                self.sidechain_number,
                &str_hash_prev[str_hash_prev.len() - 8..],
            )
            .await
            .map_err(plain_drivechain::Error::from)?;
        bitcoin::Txid::from_str(value["txid"]["txid"].as_str().ok_or(Error::InvalidJson)?)
            .map_err(plain_drivechain::Error::from)?;
        assert_eq!(header.merkle_root, body.compute_merkle_root());
        self.block = Some((header, body));
        Ok(())
    }

    pub async fn confirm_bmm(&mut self) -> Result<Option<(Header, Body)>, Error> {
        if let Some((header, body)) = self.block.clone() {
            self.drivechain.verify_bmm(&header).await?;
            self.block = None;
            return Ok(Some((header, body)));
        }
        Ok(None)
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("drivechain error")]
    Drivechain(#[from] plain_drivechain::Error),
    #[error("invalid jaon")]
    InvalidJson,
}
