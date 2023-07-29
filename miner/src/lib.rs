use bitcoin::hashes::Hash as _;
use ddk_drivechain::Drivechain;
use ddk_types::*;
use jsonrpsee::core::Serialize;
use std::str::FromStr as _;

pub use ddk_drivechain::MainClient;

#[derive(Clone)]
pub struct Miner<A, C> {
    pub drivechain: Drivechain<C>,
    block: Option<(Header, Body<A, C>)>,
    sidechain_number: u32,
}

impl<A: Clone, C: Clone + GetValue + Serialize> Miner<A, C> {
    pub fn new(sidechain_number: u32, host: &str, port: u32) -> Result<Self, Error> {
        let drivechain = Drivechain::new(sidechain_number, host, port)?;
        Ok(Self {
            drivechain,
            sidechain_number,
            block: None,
        })
    }

    pub async fn generate(&self) -> Result<(), Error> {
        self.drivechain
            .client
            .generate(1)
            .await
            .map_err(ddk_drivechain::Error::from)?;
        Ok(())
    }

    pub async fn attempt_bmm(
        &mut self,
        amount: u64,
        height: u32,
        header: Header,
        body: Body<A, C>,
    ) -> Result<(), Error> {
        let str_hash_prev = header.prev_main_hash.to_string();
        let critical_hash: [u8; 32] = header.hash().into();
        let critical_hash = bitcoin::BlockHash::from_byte_array(critical_hash);
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
            .map_err(ddk_drivechain::Error::from)?;
        bitcoin::Txid::from_str(value["txid"]["txid"].as_str().ok_or(Error::InvalidJson)?)
            .map_err(ddk_drivechain::Error::from)?;
        assert_eq!(header.merkle_root, body.compute_merkle_root());
        self.block = Some((header, body));
        Ok(())
    }

    pub async fn confirm_bmm(&mut self) -> Result<Option<(Header, Body<A, C>)>, Error> {
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
    Drivechain(#[from] ddk_drivechain::Error),
    #[error("invalid jaon")]
    InvalidJson,
}
