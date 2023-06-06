pub use sdk_authorization_ed25519_dalek;
use sdk_authorization_ed25519_dalek::*;
pub use sdk_types;
pub use sdk_types::bitcoin;
use sdk_types::{BlockHash, MerkleRoot};

use serde::{Deserialize, Serialize};

// Replace () with a type (usually an enum) for output data specific for your sidechain.
pub type Output = sdk_types::Output<()>;
pub type Transaction = sdk_types::Transaction<()>;
pub type FilledTransaction = sdk_types::FilledTransaction<()>;
pub type AuthorizedTransaction = sdk_types::AuthorizedTransaction<Authorization, ()>;
pub type Body = sdk_types::Body<Authorization, ()>;

#[derive(Serialize, Deserialize)]
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
