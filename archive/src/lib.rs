use heed::types::*;
use heed::{Database, RoTxn, RwTxn};
use plain_types::bitcoin::hashes::Hash;
use plain_types::sdk_types::BlockHash;
use plain_types::*;
use sdk_types::hash;

#[derive(Clone)]
pub struct Archive {
    // Block height to header.
    headers: Database<OwnedType<u32>, SerdeBincode<Header>>,
    bodies: Database<OwnedType<u32>, SerdeBincode<Body>>,
    hash_to_height: Database<OwnedType<[u8; 32]>, OwnedType<u32>>,
}

impl Archive {
    pub const NUM_DBS: u32 = 3;

    pub fn new(env: &heed::Env) -> Result<Self, Error> {
        let headers = env.create_database(Some("headers"))?;
        let bodies = env.create_database(Some("bodies"))?;
        let hash_to_height = env.create_database(Some("hash_to_height"))?;
        Ok(Self {
            headers,
            bodies,
            hash_to_height,
        })
    }

    pub fn get_best_hash(&self, txn: &RoTxn) -> Result<BlockHash, Error> {
        let best_hash = match self.headers.last(txn)? {
            Some((_, header)) => hash(&header).into(),
            None => [0; 32].into(),
        };
        Ok(best_hash)
    }

    pub fn get_height(&self, txn: &RoTxn) -> Result<u32, Error> {
        let height = match self.headers.last(txn)? {
            Some((height, _)) => height,
            None => 0,
        };
        Ok(height)
    }

    pub fn put_body(&self, txn: &mut RwTxn, header: &Header, body: &Body) -> Result<(), Error> {
        if header.merkle_root != body.compute_merkle_root() {
            return Err(Error::InvalidMerkleRoot);
        }
        let hash = header.hash();
        let height = self
            .hash_to_height
            .get(txn, &hash.into())?
            .ok_or(Error::NoHeader(hash))?;
        self.bodies.put(txn, &height, body)?;
        Ok(())
    }

    pub fn append_header(&self, txn: &mut RwTxn, header: &Header) -> Result<(), Error> {
        let height = self.get_height(txn)?;
        let best_hash = self.get_best_hash(txn)?;
        if header.prev_side_hash != best_hash {
            return Err(Error::InvalidPrevSideHash);
        }
        self.headers.put(txn, &(height + 1), header)?;
        self.hash_to_height
            .put(txn, &header.hash().into(), &(height + 1))?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("heed error")]
    Heed(#[from] heed::Error),
    #[error("invalid previous side hash")]
    InvalidPrevSideHash,
    #[error("invalid merkle root")]
    InvalidMerkleRoot,
    #[error("no header with hash {0}")]
    NoHeader(BlockHash),
}
