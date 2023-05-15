use heed::types::*;
use heed::{Database, RoTxn, RwTxn};
use plain_types::{sdk_authorization_ed25519_dalek, sdk_types};
use plain_types::{sdk_types::OutPoint, *};
use sdk_types::GetValue as _;

struct State {
    pub utxos: Database<SerdeBincode<OutPoint>, SerdeBincode<Output>>,
}

impl State {
    pub fn new(env: &heed::Env) -> Result<Self, Error> {
        let utxos = env.create_database(Some("utxos"))?;
        Ok(State { utxos })
    }

    pub fn validate_body(&self, txn: &RoTxn, body: &Body) -> Result<(), Error> {
        let mut coinbase_value: u64 = 0;
        for output in &body.coinbase {
            coinbase_value += output.get_value();
        }
        let mut total_fees: u64 = 0;
        for transaction in &body.transactions {
            let mut value_in: u64 = 0;
            for input in &transaction.inputs {
                let spent_utxo = self
                    .utxos
                    .get(txn, input)?
                    .ok_or(Error::NoUtxo { outpoint: *input })?;
                value_in += spent_utxo.get_value();
            }
            let mut value_out: u64 = 0;
            for output in &transaction.outputs {
                value_out += output.get_value();
            }
            if value_out > value_in {
                return Err(Error::NotEnoughValueIn);
            }
            total_fees += value_in - value_out;
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
}
