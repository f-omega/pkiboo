use clap::ValueEnum;
use itertools::Itertools;
use openssl::pkey::{PKey, Private};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::error::Error;

mod backups;
mod cli;
mod create;
mod list;
mod meta;
mod show;
pub(crate) mod split;
mod verify;

pub use cli::{Args, main};

pub struct PrivateKey {
    pub algo: Algorithm,
    pub pkey: PKey<Private>,
}
#[derive(Clone)]
pub struct Signature {
    bytes: Vec<u8>,
}

/// Return default message digest
pub fn signing_message_digest() -> openssl::hash::MessageDigest {
    openssl::hash::MessageDigest::sha512()
}

impl PrivateKey {
    fn serialize_to_pem(&self) -> Result<secrecy::SecretBox<[u8]>, Box<dyn Error>> {
        let pem = self.pkey.private_key_to_pem_pkcs8()?;
        Ok(secrecy::SecretBox::from(pem))
    }

    pub fn sign(&self, data: secrecy::SecretBox<Vec<u8>>) -> Result<Signature, Box<dyn Error>> {
        let mut signer = openssl::sign::Signer::new(signing_message_digest(), &self.pkey)?;
        signer.update(data.expose_secret())?;
        let bytes = signer.sign_to_vec()?;
        Ok(Signature { bytes })
    }
}

impl Signature {
    /// Verify this signature against the exact bytes that were originally
    /// signed.
    pub fn verify(
        &self,
        public_key: &openssl::pkey::PKey<openssl::pkey::Public>,
        data: &[u8],
    ) -> Result<bool, Box<dyn Error>> {
        let mut verifier = openssl::sign::Verifier::new(signing_message_digest(), public_key)
            .map_err(|error| format!("Could not create signature verifier: {error:?}"))?;

        verifier
            .update(data)
            .map_err(|error| format!("Could not provide signed data to OpenSSL: {error:?}"))?;

        verifier
            .verify(&self.bytes)
            .map_err(|error| format!("Could not verify signature with OpenSSL: {error:?}").into())
    }
}

impl ToString for Signature {
    fn to_string(&self) -> String {
        self.bytes
            .iter()
            .map(|c| format!("{:02x}", *c as u32))
            .collect()
    }
}

impl TryInto<Signature> for String {
    type Error = &'static str;

    fn try_into(self) -> Result<Signature, Self::Error> {
        let bytes = self
            .chars()
            .map(|c| c as u8)
            .chunks(2)
            .into_iter()
            .map(|a| {
                let c: [u8; 2] = a
                    .collect_array()
                    .ok_or("Hex-encoded string should have a length that is a multiple of two")?;
                u8::from_str_radix(
                    std::str::from_utf8(&c).map_err(|_| "Invalid character encoding")?,
                    16,
                )
                .map_err(|_| "Invalid hex digit")
            })
            .collect::<Result<Vec<u8>, _>>()?;
        Ok(Signature { bytes })
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.try_into() {
            Err(_) => Err(serde::de::Error::custom("Bad signature provided")),
            Ok(x) => Ok(x),
        }
    }
}

/// A key that has been loaded into memory
pub struct LoadedKey {
    pub pkey: PrivateKey,
    pub key: crate::pkiboo::Key,
}

impl LoadedKey {
    pub fn new(pkey: PrivateKey, key: crate::pkiboo::Key) -> Self {
        LoadedKey { pkey, key }
    }


    pub async fn save_to_media(
        &self,
        mf: &mut crate::media::OpenManifest,
    ) -> Result<(), Box<dyn Error>> {
        let secret = secrecy::SecretBox::new(Box::new(
            self.pkey.serialize_to_pem()?.expose_secret().to_vec(),
        ));
        mf.write_file(self.key.key_path(), self, secret).await
    }

    pub async fn replace_on_media(
        &self,
        mf: &mut crate::media::OpenManifest,
    ) -> Result<(), Box<dyn Error>> {
        let secret = secrecy::SecretBox::new(Box::new(
            self.pkey.serialize_to_pem()?.expose_secret().to_vec(),
        ));
        mf.replace_file(self.key.key_path(), self, secret).await
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RsaSpec {
    bits: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EcdsaSpec {
    curve: EcdsaCurve,
}

#[derive(Clone, Serialize, Deserialize, clap::Args)]
pub struct Ed25519Spec {}

#[derive(Clone, Serialize, Deserialize, ValueEnum, Debug)]
pub enum EcdsaCurve {
    P256,
    P384,
    P521,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Algorithm {
    RSA(RsaSpec),
    ECDSA(EcdsaSpec),
    ED25519(Ed25519Spec),
}

impl Algorithm {
    pub fn generate_key(&self) -> Result<PrivateKey, Box<dyn Error>> {
        match self {
            Algorithm::RSA(spec) => {
                let rsa = openssl::rsa::Rsa::generate(spec.bits as u32)?;
                Ok(PrivateKey {
                    pkey: PKey::from_rsa(rsa)?,
                    algo: self.clone(),
                })
            }
            Algorithm::ECDSA(curve) => {
                let nid = match curve.curve {
                    EcdsaCurve::P256 => openssl::nid::Nid::X9_62_PRIME256V1,
                    EcdsaCurve::P384 => openssl::nid::Nid::SECP384R1,
                    EcdsaCurve::P521 => openssl::nid::Nid::SECP521R1,
                };
                let ec_group = openssl::ec::EcGroup::from_curve_name(nid)?;
                let ec_key = openssl::ec::EcKey::generate(&ec_group)?;
                Ok(PrivateKey {
                    pkey: PKey::from_ec_key(ec_key)?,
                    algo: self.clone(),
                })
            }
            Algorithm::ED25519(_spec) => Ok(PrivateKey {
                pkey: PKey::generate_ed25519()?,
                algo: self.clone(),
            }),
        }
    }
}

impl std::fmt::Display for Algorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Algorithm::RSA(spec) => write!(f, "{}-bit RSA", spec.bits),
            Algorithm::ECDSA(spec) => write!(f, "ECDSA curve {:?}", spec.curve),
            Algorithm::ED25519(_spec) => write!(f, "ED25519 curve"),
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
pub enum AlgorithmKind {
    RSA,
    ECDSA,
    ED25519,
}

#[derive(clap::Args)]
pub struct AlgorithmArgs {
    #[arg(long = "type", short = 't', value_enum)]
    kind: AlgorithmKind,

    #[arg(long = "rsa-bits", short = 'B')]
    rsa_bits: Option<usize>,
    #[arg(long)]
    ecdsa_curve: Option<EcdsaCurve>,
}

impl AlgorithmArgs {
    fn to_algorithm(&self) -> Option<Algorithm> {
        match self.kind {
            AlgorithmKind::RSA => self
                .rsa_bits
                .as_ref()
                .map(|s| Algorithm::RSA(RsaSpec { bits: *s })),
            AlgorithmKind::ECDSA => self
                .ecdsa_curve
                .as_ref()
                .map(|s| Algorithm::ECDSA(EcdsaSpec { curve: s.clone() })),
            AlgorithmKind::ED25519 => Some(Algorithm::ED25519(Ed25519Spec {})),
        }
    }
}
