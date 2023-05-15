pub use sdk_authorization_ed25519_dalek;
use sdk_authorization_ed25519_dalek::*;
pub use sdk_types;

pub type Output = sdk_types::Output<()>;
pub type Transaction = sdk_types::Transaction<()>;
pub type AuthorizedTransaction = sdk_types::AuthorizedTransaction<Authorization, ()>;
pub type Body = sdk_types::Body<Authorization, ()>;
