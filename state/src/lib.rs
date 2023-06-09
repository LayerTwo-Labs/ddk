// TODO: Turn this into a cargo generate template.
pub use heed;
use heed::types::*;
use heed::{Database, RoTxn, RwTxn};
use plain_types::{sdk_authorization_ed25519_dalek, sdk_types, TwoWayPegData};
use plain_types::{sdk_types::OutPoint, *};
use sdk_types::GetValue as _;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub struct State {
    pub utxos: Database<SerdeBincode<OutPoint>, SerdeBincode<Output>>,
    pub last_deposit_block: Database<OwnedType<u32>, SerdeBincode<bitcoin::BlockHash>>,
}

impl State {
    pub const NUM_DBS: u32 = 2;

    pub fn new(env: &heed::Env) -> Result<Self, Error> {
        let utxos = env.create_database(Some("utxos"))?;
        let last_deposit_block = env.create_database(Some("last_deposit_block"))?;
        Ok(Self {
            utxos,
            last_deposit_block,
        })
    }

    pub fn get_utxos(&self, txn: &RoTxn) -> Result<HashMap<OutPoint, Output>, Error> {
        let mut utxos = HashMap::new();
        for item in self.utxos.iter(txn)? {
            let (outpoint, output) = item?;
            utxos.insert(outpoint, output);
        }
        Ok(utxos)
    }

    pub fn validate_transaction(
        &self,
        txn: &RoTxn,
        transaction: &AuthorizedTransaction,
    ) -> Result<u64, Error> {
        let filled_transaction = self.fill_transaction(txn, &transaction.transaction)?;
        for (authorization, spent_utxo) in transaction
            .authorizations
            .iter()
            .zip(filled_transaction.spent_utxos.iter())
        {
            if sdk_types::Address::from(authorization.public_key.to_bytes()) != spent_utxo.address {
                return Err(Error::WrongPubKeyForAddress);
            }
        }
        sdk_authorization_ed25519_dalek::verify_authorized_transaction(&transaction)?;
        let fee = self.validate_filled_transaction(&filled_transaction)?;
        Ok(fee)
    }

    fn fill_transaction(
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

    fn validate_filled_transaction(&self, transaction: &FilledTransaction) -> Result<u64, Error> {
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

    pub fn validate_body(&self, txn: &RoTxn, body: &Body) -> Result<u64, Error> {
        let mut coinbase_value: u64 = 0;
        for output in &body.coinbase {
            coinbase_value += output.get_value();
        }
        let mut total_fees: u64 = 0;
        let mut spent_utxos = HashSet::new();
        let filled_transactions: Vec<_> = body
            .transactions
            .iter()
            .map(|t| self.fill_transaction(txn, t))
            .collect::<Result<_, _>>()?;
        for filled_transaction in &filled_transactions {
            for input in &filled_transaction.transaction.inputs {
                if spent_utxos.contains(input) {
                    return Err(Error::UtxoDoubleSpent);
                }
                spent_utxos.insert(*input);
            }
            total_fees += self.validate_filled_transaction(filled_transaction)?;
        }
        if coinbase_value > total_fees {
            return Err(Error::NotEnoughFees);
        }
        let spent_utxos = filled_transactions
            .iter()
            .flat_map(|t| t.spent_utxos.iter());
        for (authorization, spent_utxo) in body.authorizations.iter().zip(spent_utxos) {
            if sdk_types::Address::from(authorization.public_key.to_bytes()) != spent_utxo.address {
                return Err(Error::WrongPubKeyForAddress);
            }
        }
        Ok(total_fees)
    }

    pub fn get_last_deposit_block_hash(
        &self,
        txn: &RoTxn,
    ) -> Result<Option<bitcoin::BlockHash>, Error> {
        Ok(self.last_deposit_block.get(&txn, &0)?)
    }

    pub fn connect_two_way_peg_data(
        &self,
        txn: &mut RwTxn,
        two_way_peg_data: &TwoWayPegData,
    ) -> Result<(), Error> {
        // Handle deposits.
        if let Some(deposit_block_hash) = two_way_peg_data.deposit_block_hash {
            self.last_deposit_block.put(txn, &0, &deposit_block_hash)?;
        }
        for (outpoint, deposit) in &two_way_peg_data.deposits {
            self.utxos.put(txn, outpoint, deposit)?;
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
    #[error("wrong public key for address")]
    WrongPubKeyForAddress,
}
