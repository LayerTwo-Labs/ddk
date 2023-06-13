use ed25519_dalek_bip32::*;
use heed::types::*;
use heed::{Database, RoTxn, RwTxn};
use plain_types::sdk_authorization_ed25519_dalek::{get_address, Authorization};
use plain_types::sdk_types::{Address, GetValue, OutPoint};
use plain_types::{sdk_types::GetValue as _, *};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct Wallet {
    env: heed::Env,
    // FIXME: Don't store the seed in plaintext.
    seed: Database<OwnedType<u32>, OwnedType<[u8; 64]>>,
    pub address_to_index: Database<SerdeBincode<Address>, OwnedType<u32>>,
    pub index_to_address: Database<OwnedType<u32>, SerdeBincode<Address>>,
    pub outpoint_to_address: Database<SerdeBincode<OutPoint>, SerdeBincode<Address>>,
    pub utxos: Database<SerdeBincode<OutPoint>, SerdeBincode<Output>>,
}

impl Wallet {
    pub const NUM_DBS: u32 = 5;

    pub fn new(seed: [u8; 64], path: &Path) -> Result<Self, Error> {
        std::fs::create_dir_all(path)?;
        let env = heed::EnvOpenOptions::new()
            .map_size(10 * 1024 * 1024) // 10MB
            .max_dbs(Self::NUM_DBS)
            .open(path)?;

        let seed_db = env.create_database(Some("seed"))?;
        let address_to_index = env.create_database(Some("address_to_index"))?;
        let index_to_address = env.create_database(Some("index_to_address"))?;
        let outpoint_to_address = env.create_database(Some("outpoint_to_address"))?;
        let utxos = env.create_database(Some("utxos"))?;

        let mut txn = env.write_txn()?;
        seed_db.put(&mut txn, &0, &seed)?;
        txn.commit()?;
        Ok(Self {
            env,
            seed: seed_db,
            address_to_index,
            index_to_address,
            outpoint_to_address,
            utxos,
        })
    }

    pub fn select_coins() -> Result<HashMap<OutPoint, Output>, Error> {
        todo!();
    }

    pub fn put_utxos(&self, utxos: &HashMap<OutPoint, Output>) -> Result<(), Error> {
        let mut txn = self.env.write_txn()?;
        for (outpoint, output) in utxos {
            self.utxos.put(&mut txn, outpoint, output)?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_balance(&self) -> Result<u64, Error> {
        let mut balance: u64 = 0;
        let txn = self.env.read_txn()?;
        for item in self.utxos.iter(&txn)? {
            let (_, utxo) = item?;
            balance += utxo.get_value();
        }
        Ok(balance)
    }

    pub fn get_addresses(&self) -> Result<HashSet<Address>, Error> {
        let txn = self.env.read_txn()?;
        let mut addresses = HashSet::new();
        for item in self.index_to_address.iter(&txn)? {
            let (_, address) = item?;
            addresses.insert(address);
        }
        Ok(addresses)
    }

    pub fn authorize(
        &self,
        txn: &RoTxn,
        transaction: Transaction,
    ) -> Result<AuthorizedTransaction, Error> {
        let mut authorizations = vec![];
        for input in &transaction.inputs {
            let spent_utxo = self.utxos.get(txn, input)?.ok_or(Error::NoUtxo)?;
            let index =
                self.address_to_index
                    .get(txn, &spent_utxo.address)?
                    .ok_or(Error::NoIndex {
                        address: spent_utxo.address,
                    })?;
            let keypair = self.get_keypair(txn, index)?;
            let signature = sdk_authorization_ed25519_dalek::sign(&keypair, &transaction)?;
            authorizations.push(Authorization {
                public_key: keypair.public,
                signature,
            });
        }
        Ok(AuthorizedTransaction {
            authorizations,
            transaction,
        })
    }

    pub fn get_new_address(&self) -> Result<Address, Error> {
        let mut txn = self.env.write_txn()?;
        let (last_index, _) = self
            .index_to_address
            .last(&txn)?
            .unwrap_or((0, [0; 32].into()));
        let index = last_index + 1;
        let keypair = self.get_keypair(&txn, index)?;
        let address = get_address(&keypair.public);
        self.index_to_address.put(&mut txn, &index, &address)?;
        self.address_to_index.put(&mut txn, &address, &index)?;
        txn.commit()?;
        Ok(address)
    }

    fn get_keypair(&self, txn: &RoTxn, index: u32) -> Result<ed25519_dalek::Keypair, Error> {
        let seed = self.seed.get(txn, &0)?.ok_or(Error::NoSeed)?;
        let xpriv = ExtendedSecretKey::from_seed(&seed)?;
        let derivation_path = DerivationPath::new([
            ChildIndex::Hardened(1),
            ChildIndex::Hardened(0),
            ChildIndex::Hardened(0),
            ChildIndex::Hardened(index),
        ]);
        let child = xpriv.derive(&derivation_path)?;
        let public = child.public_key();
        let secret = child.secret_key;
        Ok(ed25519_dalek::Keypair { secret, public })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("heed error")]
    Heed(#[from] heed::Error),
    #[error("bip32 error")]
    Bip32(#[from] ed25519_dalek_bip32::Error),
    #[error("address {address} does not exist")]
    AddressDoesNotExist { address: sdk_types::Address },
    #[error("utxo doesn't exist")]
    NoUtxo,
    #[error("wallet doesn't have a seed")]
    NoSeed,
    #[error("no index for address {address}")]
    NoIndex { address: Address },
    #[error("authorization error")]
    Authorization(#[from] sdk_authorization_ed25519_dalek::Error),
    #[error("io error")]
    Io(#[from] std::io::Error),
}
