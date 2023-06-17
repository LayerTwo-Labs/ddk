pub use sdk_authorization_ed25519_dalek;
use sdk_authorization_ed25519_dalek::*;
pub use sdk_types;
pub use sdk_types::bitcoin;
use sdk_types::{BlockHash, MerkleRoot, OutPoint};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashMap};

// Replace () with a type (usually an enum) for output data specific for your sidechain.
pub type Output = sdk_types::Output<()>;
pub type Transaction = sdk_types::Transaction<()>;
pub type FilledTransaction = sdk_types::FilledTransaction<()>;
pub type AuthorizedTransaction = sdk_types::AuthorizedTransaction<Authorization, ()>;
pub type Body = sdk_types::Body<Authorization, ()>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub merkle_root: MerkleRoot,
    pub prev_side_hash: BlockHash,
    pub prev_main_hash: bitcoin::BlockHash,
}

impl Header {
    pub fn hash(&self) -> BlockHash {
        sdk_types::hash(self).into()
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum WithdrawalBundleStatus {
    Failed,
    Confirmed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalBundle {
    pub spent_utxos: HashMap<sdk_types::OutPoint, Output>,
    pub transaction: bitcoin::Transaction,
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TwoWayPegData {
    pub deposits: HashMap<sdk_types::OutPoint, Output>,
    pub deposit_block_hash: Option<bitcoin::BlockHash>,
    pub bundle_statuses: HashMap<bitcoin::Txid, WithdrawalBundleStatus>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DisconnectData {
    pub spent_utxos: HashMap<sdk_types::OutPoint, Output>,
    pub deposits: Vec<sdk_types::OutPoint>,
    pub pending_bundles: Vec<bitcoin::Txid>,
    pub spent_bundles: HashMap<bitcoin::Txid, Vec<sdk_types::OutPoint>>,
    pub spent_withdrawals: HashMap<sdk_types::OutPoint, Output>,
    pub failed_withdrawals: Vec<bitcoin::Txid>,
}

#[derive(Eq, PartialEq, Clone, Debug)]
pub struct AggregatedWithdrawal {
    pub spent_utxos: HashMap<OutPoint, Output>,
    pub main_address: bitcoin::Address,
    pub value: u64,
    pub main_fee: u64,
}

impl Ord for AggregatedWithdrawal {
    fn cmp(&self, other: &Self) -> Ordering {
        if self == other {
            Ordering::Equal
        } else if self.main_fee > other.main_fee
            || self.value > other.value
            || self.main_address > other.main_address
        {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    }
}

impl PartialOrd for AggregatedWithdrawal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
