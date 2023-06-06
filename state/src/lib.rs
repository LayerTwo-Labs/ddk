// TODO: Turn this into a cargo generate template.
pub use heed;
use heed::types::*;
use heed::{Database, RoTxn, RwTxn};
use plain_types::{sdk_authorization_ed25519_dalek, sdk_types};
use plain_types::{sdk_types::OutPoint, *};
use sdk_types::GetValue as _;
use std::collections::HashSet;

#[derive(Clone)]
pub struct State {
    pub utxos: Database<SerdeBincode<OutPoint>, SerdeBincode<Output>>,
}

impl State {
    pub const NUM_DBS: u32 = 1;

    pub fn new(env: &heed::Env) -> Result<Self, Error> {
        let utxos = env.create_database(Some("utxos"))?;
        Ok(Self { utxos })
    }

    pub fn fill_transaction(
        &self,
        txn: &RoTxn,
        transaction: &Transaction,
    ) -> Result<FilledTransaction, Error> {
        let mut spent_utxos = vec![];
        for input in &transaction.inputs {
            let utxo = self
                .utxos
                .get(txn, input)?
                .ok_or(Error::NoUtxo { outpoint: *input })?;
            spent_utxos.push(utxo);
        }
        Ok(FilledTransaction {
            spent_utxos,
            transaction: transaction.clone(),
        })
    }

    pub fn validate_filled_transaction(
        &self,
        transaction: &FilledTransaction,
    ) -> Result<u64, Error> {
        let mut value_in: u64 = 0;
        let mut value_out: u64 = 0;
        for utxo in &transaction.spent_utxos {
            value_in += utxo.get_value();
        }
        for output in &transaction.transaction.outputs {
            value_out += output.get_value();
        }
        if value_out > value_in {
            return Err(Error::NotEnoughValueIn);
        }
        Ok(value_in - value_out)
    }

    pub fn validate_body(&self, txn: &RoTxn, body: &Body) -> Result<(), Error> {
        let mut coinbase_value: u64 = 0;
        for output in &body.coinbase {
            coinbase_value += output.get_value();
        }
        let mut total_fees: u64 = 0;
        let mut spent_utxos = HashSet::new();
        for transaction in &body.transactions {
            for input in &transaction.inputs {
                if spent_utxos.contains(input) {
                    return Err(Error::UtxoDoubleSpent);
                }
                spent_utxos.insert(*input);
            }
            let transaction = self.fill_transaction(txn, transaction)?;
            total_fees += self.validate_filled_transaction(&transaction)?;
        }
        if coinbase_value > total_fees {
            return Err(Error::NotEnoughFees);
        }
        Ok(())
    }

    pub fn connect_body(&self, txn: &mut RwTxn, body: &Body) -> Result<(), Error> {
        let merkle_root = body.compute_merkle_root();
        for (vout, output) in body.coinbase.iter().enumerate() {
            let outpoint = OutPoint::Coinbase {
                merkle_root,
                vout: vout as u32,
            };
            self.utxos.put(txn, &outpoint, output)?;
        }
        for transaction in &body.transactions {
            let txid = transaction.txid();
            for input in &transaction.inputs {
                self.utxos.delete(txn, input)?;
            }
            for (vout, output) in transaction.outputs.iter().enumerate() {
                let outpoint = OutPoint::Regular {
                    txid,
                    vout: vout as u32,
                };
                self.utxos.put(txn, &outpoint, output)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("authorization error")]
    Authorization(#[from] sdk_authorization_ed25519_dalek::Error),
    #[error("sdk error")]
    Sdk(#[from] sdk_types::Error),
    #[error("heed error")]
    Heed(#[from] heed::Error),
    #[error("utxo {outpoint} doesn't exist")]
    NoUtxo { outpoint: OutPoint },
    // TODO: Write better errors!
    #[error("value in is less than value out")]
    NotEnoughValueIn,
    #[error("total fees less than coinbase value")]
    NotEnoughFees,
    #[error("utxo double spent")]
    UtxoDoubleSpent,
}
