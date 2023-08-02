pub use crate::types::address::*;
pub use crate::types::hashes::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// remove once ! is stabilized
// see tracking issue: https://github.com/rust-lang/rust/issues/35121
use never_type::Never;

#[derive(Hash, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutPoint {
    // Created by transactions.
    Regular { txid: Txid, vout: u32 },
    // Created by block bodies.
    Coinbase { merkle_root: MerkleRoot, vout: u32 },
    // Created by mainchain deposits.
    Deposit(bitcoin::OutPoint),
}

impl std::fmt::Display for OutPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Regular { txid, vout } => write!(f, "regular {txid} {vout}"),
            Self::Coinbase { merkle_root, vout } => write!(f, "coinbase {merkle_root} {vout}"),
            Self::Deposit(bitcoin::OutPoint { txid, vout }) => write!(f, "deposit {txid} {vout}"),
        }
    }
}

/// The default custom tx output.
/// The `Never` type is used to express that
/// the custom output variant is not used by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultCustomTxOutput(pub Never);

impl Serialize for DefaultCustomTxOutput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_unit_struct("DefaultCustomTxOutput")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content<CustomTxOutput = DefaultCustomTxOutput> {
    Custom(CustomTxOutput),
    Value(u64),
    Withdrawal {
        value: u64,
        main_fee: u64,
        main_address: bitcoin::Address<bitcoin::address::NetworkUnchecked>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output<C> {
    pub address: Address,
    pub content: Content<C>,
}

impl<C> Content<C> {
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom(_))
    }
    pub fn is_value(&self) -> bool {
        matches!(self, Self::Value(_))
    }
    pub fn is_withdrawal(&self) -> bool {
        matches!(self, Self::Withdrawal { .. })
    }
}

impl<C> GetAddress for Output<C> {
    #[inline(always)]
    fn get_address(&self) -> Address {
        self.address
    }
}

impl<C: GetValue> GetValue for Output<C> {
    #[inline(always)]
    fn get_value(&self) -> u64 {
        self.content.get_value()
    }
}

impl<C: GetValue> GetValue for Content<C> {
    #[inline(always)]
    fn get_value(&self) -> u64 {
        match self {
            Self::Custom(custom) => custom.get_value(),
            Self::Value(value) => *value,
            Self::Withdrawal { value, .. } => *value,
        }
    }
}

/// The default tx extension
pub(crate) type DefaultTxExtension = ();

/// `CustomTxExtension` is used to add custom data to a transaction.
/// `CustomTxOutput` is used to add support for custom output kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction<
    CustomTxExtension = DefaultTxExtension,
    CustomTxOutput = DefaultCustomTxOutput,
> {
    pub inputs: Vec<OutPoint>,
    pub outputs: Vec<Output<CustomTxOutput>>,
    pub extension: CustomTxExtension,
}

impl<CustomTxExtension, CustomTxOutput> Transaction<CustomTxExtension, CustomTxOutput>
where
    CustomTxExtension: Serialize,
    CustomTxOutput: Serialize,
{
    pub fn txid(&self) -> Txid {
        hash(self).into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilledTransaction<
    CustomTxExtension = DefaultTxExtension,
    CustomTxOutput = DefaultCustomTxOutput,
> {
    pub transaction: Transaction<CustomTxExtension, CustomTxOutput>,
    pub spent_utxos: Vec<Output<CustomTxOutput>>,
}

impl<CustomTxExtension, CustomTxOutput> FilledTransaction<CustomTxExtension, CustomTxOutput>
where
    CustomTxOutput: GetValue,
{
    pub fn get_value_in(&self) -> u64 {
        self.spent_utxos.iter().map(GetValue::get_value).sum()
    }

    pub fn get_value_out(&self) -> u64 {
        self.transaction
            .outputs
            .iter()
            .map(GetValue::get_value)
            .sum()
    }

    pub fn get_fee(&self) -> Option<u64> {
        let value_in = self.get_value_in();
        let value_out = self.get_value_out();
        if value_in < value_out {
            None
        } else {
            Some(value_in - value_out)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedTransaction<
    A,
    CustomTxExtension = DefaultTxExtension,
    CustomTxOutput = DefaultCustomTxOutput,
> {
    pub transaction: Transaction<CustomTxExtension, CustomTxOutput>,
    /// Authorization is called witness in Bitcoin.
    pub authorizations: Vec<A>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body<A, CustomTxExtension = DefaultTxExtension, CustomTxOutput = DefaultCustomTxOutput> {
    pub coinbase: Vec<Output<CustomTxOutput>>,
    pub transactions: Vec<Transaction<CustomTxExtension, CustomTxOutput>>,
    pub authorizations: Vec<A>,
}

impl<A, CustomTxExtension, CustomTxOutput> Body<A, CustomTxExtension, CustomTxOutput>
where
    CustomTxExtension: Serialize,
    CustomTxOutput: Clone + GetValue + Serialize,
{
    pub fn new(
        authorized_transactions: Vec<AuthorizedTransaction<A, CustomTxExtension, CustomTxOutput>>,
        coinbase: Vec<Output<CustomTxOutput>>,
    ) -> Self {
        let mut authorizations = Vec::with_capacity(
            authorized_transactions
                .iter()
                .map(|t| t.transaction.inputs.len())
                .sum(),
        );
        let mut transactions = Vec::with_capacity(authorized_transactions.len());
        for at in authorized_transactions.into_iter() {
            authorizations.extend(at.authorizations);
            transactions.push(at.transaction);
        }
        Self {
            coinbase,
            transactions,
            authorizations,
        }
    }

    pub fn compute_merkle_root(&self) -> MerkleRoot {
        // FIXME: Compute actual merkle root instead of just a hash.
        hash(&(&self.coinbase, &self.transactions)).into()
    }

    pub fn get_inputs(&self) -> Vec<OutPoint> {
        self.transactions
            .iter()
            .flat_map(|tx| tx.inputs.iter())
            .copied()
            .collect()
    }

    pub fn get_outputs(&self) -> HashMap<OutPoint, Output<CustomTxOutput>> {
        let mut outputs = HashMap::new();
        let merkle_root = self.compute_merkle_root();
        for (vout, output) in self.coinbase.iter().enumerate() {
            let vout = vout as u32;
            let outpoint = OutPoint::Coinbase { merkle_root, vout };
            outputs.insert(outpoint, output.clone());
        }
        for transaction in &self.transactions {
            let txid = transaction.txid();
            for (vout, output) in transaction.outputs.iter().enumerate() {
                let vout = vout as u32;
                let outpoint = OutPoint::Regular { txid, vout };
                outputs.insert(outpoint, output.clone());
            }
        }
        outputs
    }

    pub fn get_coinbase_value(&self) -> u64 {
        self.coinbase.iter().map(|output| output.get_value()).sum()
    }
}

pub trait GetAddress {
    fn get_address(&self) -> Address;
}

pub trait GetValue {
    fn get_value(&self) -> u64;
}

impl GetValue for () {
    fn get_value(&self) -> u64 {
        0
    }
}

impl GetValue for Never {
    fn get_value(&self) -> u64 {
        0
    }
}

pub trait Verify<CustomTxExtension, CustomTxOutput> {
    type Error;
    fn verify_transaction(
        transaction: &AuthorizedTransaction<Self, CustomTxExtension, CustomTxOutput>,
    ) -> Result<(), Self::Error>
    where
        Self: Sized;
    fn verify_body(body: &Body<Self, CustomTxExtension, CustomTxOutput>) -> Result<(), Self::Error>
    where
        Self: Sized;
}
