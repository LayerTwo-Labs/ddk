use heed::types::*;
use heed::{Database, RoTxn, RwTxn};
use plain_types::{AuthorizedTransaction, Txid};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct MemPool<A, C> {
    pub transactions: Database<OwnedType<[u8; 32]>, SerdeBincode<AuthorizedTransaction<A, C>>>,
}

impl<
        A: Serialize + for<'de> Deserialize<'de> + 'static,
        C: Serialize + for<'de> Deserialize<'de> + 'static,
    > MemPool<A, C>
{
    pub const NUM_DBS: u32 = 1;

    pub fn new(env: &heed::Env) -> Result<Self, Error> {
        let transactions = env.create_database(Some("transactinos"))?;
        Ok(Self { transactions })
    }

    pub fn put(
        &self,
        txn: &mut RwTxn,
        transaction: &AuthorizedTransaction<A, C>,
    ) -> Result<(), Error> {
        println!(
            "adding transaction {} to mempool",
            transaction.transaction.txid()
        );
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
    ) -> Result<Vec<AuthorizedTransaction<A, C>>, Error> {
        let mut transactions = vec![];
        for item in self.transactions.iter(txn)?.take(number) {
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
}
