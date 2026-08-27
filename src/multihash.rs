use serde::{Serialize, Deserialize};
use std::fmt::Display;

use openssl::hash::{hash, MessageDigest};

#[derive(Clone, PartialEq)]
pub(crate) enum HashAlgorithm {
    SHA256,
    SHA512
}

impl HashAlgorithm {
    fn default_algo() -> Self {
        HashAlgorithm::SHA512
    }

    fn variants() -> &'static [&'static str] {
        return &[ "sha256", "sha512" ];
    }

    fn from_string(s: &str) -> Option<HashAlgorithm> {
        use HashAlgorithm::*;
        match s {
            "sha256" => Some(SHA256),
            "sha512" => Some(SHA512),
            _ => None
        }
    }

    fn to_string(&self) -> &'static str {
        use HashAlgorithm::*;
        match self {
            SHA256 => "sha256",
            SHA512 => "sha512"
        }
    }

    fn size(&self) -> usize {
        use HashAlgorithm::*;
        match self {
            SHA256 => 32,
            SHA512 => 64
        }
    }

    fn message_digest(&self) -> MessageDigest {
        use HashAlgorithm::*;
        match self {
            SHA256 => MessageDigest::sha256(),
            SHA512 => MessageDigest::sha512()
        }
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct MultiHash {
    kind: HashAlgorithm,
    hex_encoded: String
}

impl MultiHash {
    pub(crate) fn new(algo: HashAlgorithm, hex: String) -> Self {
        MultiHash { kind: algo, hex_encoded: hex }
    }

    pub(crate) fn valid(&self) -> Result<(), Box<dyn Display>> {
        let len = self.kind.size() * 2;
        if self.hex_encoded.len() != len {
            return Err(Box::<String>::new(format!("Algorithm {} expects {} hex digits, but got {}", self.kind.to_string(), len, self.hex_encoded.len()).into()));
        };
        if !self.hex_encoded.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Box::<String>::new("Hex digest must consist of only hex digits".into()));
        };
        Ok(())
    }

    pub(crate) fn with_default_algo<'a>(b: impl Into<&'a Vec<u8>>) -> Self {
        Self::hash(HashAlgorithm::default_algo(), b)
    }

    pub(crate) fn hash<'a>(algo: HashAlgorithm, b: impl Into<&'a Vec<u8>>) -> Self {
        let h = hash(algo.message_digest(), b.into()).unwrap();
        let hex_encoded = h.iter().map(|c| format!("{:02x}", *c as u32)).collect::<String>();
        Self { kind: algo, hex_encoded }
    }

    pub(crate) fn check<'a>(&self, b: impl Into<&'a Vec<u8>>) -> bool {
        MultiHash::hash(self.kind.clone(), b) == *self
    }
}

impl ToString for MultiHash {
    fn to_string(&self) -> String {
        format!("{}:{}", self.kind.to_string(), self.hex_encoded)
    }
}

impl Serialize for MultiHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MultiHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        let (hash, bytes) = s.split_once(|c| c == ':').ok_or(serde::de::Error::custom("multihash must have form <hashalgo>:<hexbytes>"))?;
        match HashAlgorithm::from_string(hash) {
            Some(algo) => {
                let hash = MultiHash::new(algo, bytes.into());
                hash.valid().map_err(|e| serde::de::Error::custom(e))?;
                Ok(hash)
            },
            None => Err(serde::de::Error::unknown_variant(hash, HashAlgorithm::variants()))
        }
    }
}
