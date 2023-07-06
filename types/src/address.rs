#[derive(Clone, Copy, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Address(pub [u8; 20]);

impl Address {
    pub fn to_base58(self) -> String {
        bs58::encode(self.0)
            .with_alphabet(bs58::Alphabet::BITCOIN)
            .with_check()
            .into_string()
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl From<[u8; 20]> for Address {
    fn from(other: [u8; 20]) -> Self {
        Self(other)
    }
}

impl std::str::FromStr for Address {
    type Err = bs58::decode::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let address = bs58::decode(s)
            .with_alphabet(bs58::Alphabet::BITCOIN)
            .with_check(None)
            .into_vec()?;
        assert_eq!(address.len(), 20);
        Ok(Address(address.try_into().unwrap()))
    }
}
