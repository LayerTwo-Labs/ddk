use ed25519_dalek_bip32::*;
use heed::types::*;
use heed::{Database, RoTxn, RwTxn};
use plain_types::sdk_authorization_ed25519_dalek::Authorization;
use plain_types::sdk_types::{Address, OutPoint};
use plain_types::{sdk_types::GetValue as _, *};
use std::collections::HashMap;

pub struct Wallet {
    // FIXME: Don't store the seed in plaintext.
    seed: Database<OwnedType<u32>, OwnedType<[u8; 64]>>,
    pub address_to_index: Database<SerdeBincode<Address>, OwnedType<u32>>,
    pub index_to_address: Database<OwnedType<u32>, SerdeBincode<Address>>,
    pub outpoint_to_address: Database<SerdeBincode<OutPoint>, SerdeBincode<Address>>,
    pub utxos: Database<SerdeBincode<OutPoint>, SerdeBincode<Output>>,
}

impl Wallet {
    const NUM_DBS: u32 = 5;

    pub fn new(seed: [u8; 64], env: &heed::Env) -> Result<Self, Error> {
        let seed = env.create_database(Some("seed"))?;
        let address_to_index = env.create_database(Some("address_to_index"))?;
        let index_to_address = env.create_database(Some("index_to_address"))?;
        let outpoint_to_address = env.create_database(Some("outpoint_to_address"))?;
        let utxos = env.create_database(Some("utxos"))?;
        Ok(Self {
            seed,
            address_to_index,
            index_to_address,
            outpoint_to_address,
            utxos,
        })
    }

    pub fn select_coins() -> Result<HashMap<OutPoint, Output>, Error> {
        todo!();
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
}
