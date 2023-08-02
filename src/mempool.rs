use crate::types::{
    AuthorizedTransaction, DefaultCustomTxOutput, DefaultTxExtension, OutPoint, Txid,
};
use heed::types::*;
use heed::{Database, RoTxn, RwTxn};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct MemPool<
    A,
    CustomTxExtension = DefaultTxExtension,
    CustomTxOutput = DefaultCustomTxOutput,
> {
    pub transactions: Database<
        OwnedType<[u8; 32]>,
        SerdeBincode<AuthorizedTransaction<A, CustomTxExtension, CustomTxOutput>>,
    >,
    pub spent_utxos: Database<SerdeBincode<OutPoint>, Unit>,
}

impl<A, CustomTxExtension, CustomTxOutput> MemPool<A, CustomTxExtension, CustomTxOutput>
where
    A: Serialize + for<'de> Deserialize<'de> + 'static,
    CustomTxExtension: Serialize + for<'de> Deserialize<'de> + 'static,
    CustomTxOutput: Serialize + for<'de> Deserialize<'de> + 'static,
{
    pub const NUM_DBS: u32 = 1;

    pub fn new(env: &heed::Env) -> Result<Self, Error> {
        let transactions = env.create_database(Some("transactions"))?;
        let spent_utxos = env.create_database(Some("spent_utxos"))?;
        Ok(Self {
            transactions,
            spent_utxos,
        })
    }

    pub fn put(
        &self,
        txn: &mut RwTxn,
        transaction: &AuthorizedTransaction<A, CustomTxExtension, CustomTxOutput>,
    ) -> Result<(), Error> {
        println!(
            "adding transaction {} to mempool",
            transaction.transaction.txid()
        );
        for input in &transaction.transaction.inputs {
            if self.spent_utxos.get(txn, input)?.is_some() {
                return Err(Error::UtxoDoubleSpent);
            }
            self.spent_utxos.put(txn, input, &())?;
        }
        self.transactions
            .put(txn, &transaction.transaction.txid().into(), &transaction)?;
        Ok(())
    }

    pub fn delete(&self, txn: &mut RwTxn, txid: &Txid) -> Result<(), Error> {
        self.transactions.delete(txn, txid.into())?;
        Ok(())
    }

    pub fn take(
        &self,
        txn: &RoTxn,
        number: usize,
    ) -> Result<Vec<AuthorizedTransaction<A, CustomTxExtension, CustomTxOutput>>, Error> {
        let mut transactions = vec![];
        for item in self.transactions.iter(txn)?.take(number) {
            let (_, transaction) = item?;
            transactions.push(transaction);
        }
        Ok(transactions)
    }

    pub fn take_all(
        &self,
        txn: &RoTxn,
    ) -> Result<Vec<AuthorizedTransaction<A, CustomTxExtension, CustomTxOutput>>, Error> {
        let mut transactions = vec![];
        for item in self.transactions.iter(txn)? {
            let (_, transaction) = item?;
            transactions.push(transaction);
        }
        Ok(transactions)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("heed error")]
    Heed(#[from] heed::Error),
    #[error("can't add transaction, utxo double spent")]
    UtxoDoubleSpent,
}
