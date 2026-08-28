//! Portable Pedersen-verifiable recovery shares.
//!
//! The private key is encrypted, never shared. Four random P-256 scalars form
//! a 128-byte recovery secret; each scalar is independently shared. Both
//! Feldman and hiding Pedersen commitments are serialized for interoperability.
use crate::{multihash::MultiHash, pkiboo::Paper, util::Name};
use openssl::{
    hash::MessageDigest,
    symm::{Cipher, Crypter, Mode},
};
use p256::{
    ProjectivePoint, Scalar,
    elliptic_curve::{Field, PrimeField, group::GroupEncoding},
};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::error::Error;
use vsss_rs::{
    DefaultShare, FeldmanVerifierSet, IdentifierPrimeField, PedersenResult, PedersenVerifierSet,
    ReadableShareSet, Share, ShareVerifierGroup, ValueGroup,
};

type P256Share = DefaultShare<IdentifierPrimeField<Scalar>, IdentifierPrimeField<Scalar>>;
type P256Verifier = ShareVerifierGroup<ProjectivePoint>;
type VerifierSet = Vec<P256Verifier>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShareFormatVersion {
    V1,
}
impl Serialize for ShareFormatVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(1)
    }
}
impl<'de> Deserialize<'de> for ShareFormatVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match u8::deserialize(d)? {
            1 => Ok(Self::V1),
            n => Err(serde::de::Error::custom(format!(
                "unsupported recovery share version {n}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShamirParameters {
    pub shares: u8,
    pub threshold: u8,
    pub field: String,
    pub components: u8,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncryptedPrivateKey {
    pub cipher: String,
    pub key_derivation: String,
    pub nonce: String,
    pub ciphertext: String,
    pub tag: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerifiableComponent {
    pub value: String,
    pub blinder: String,
    pub feldman_generator: String,
    pub feldman_commitments: Vec<String>,
    pub pedersen_secret_generator: String,
    pub pedersen_blinder_generator: String,
    pub pedersen_commitments: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShamirShareFile {
    pub version: ShareFormatVersion,
    /// MultiHash of public-key PEM bytes (not a certificate fingerprint).
    pub public_key: MultiHash,
    pub shamir: ShamirParameters,
    /// Evaluation point and human-visible share number.
    pub x: u8,
    pub encrypted_private_key: EncryptedPrivateKey,
    pub components: Vec<VerifiableComponent>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PaperShare {
    /// Human-facing label captured at issuance; not a cryptographic identity
    /// and allowed to differ after a managed key is renamed.
    pub key_name: String,
    pub paper_name: Name<Paper>,
    pub share: ShamirShareFile,
    pub placements: PaperSharePlacements,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PaperSharePlacements {
    /// Names of every paper share in this split, including this paper.
    pub paper: Vec<String>,
    /// Names of media holding shares in this split.
    pub storage: Vec<String>,
}

/// Generate all envelopes in memory; callers must self-test before writing.
pub fn split_private_key(
    private_pem: &[u8],
    public_key: MultiHash,
    shares: u8,
    threshold: u8,
) -> Result<Vec<ShamirShareFile>, Box<dyn Error>> {
    if threshold < 2 || threshold > shares {
        return Err("threshold must be at least 2 and no greater than the share count".into());
    }
    let mut rng = OsRng;
    let secrets = (0..4).map(|_| Scalar::random(&mut rng)).collect::<Vec<_>>();
    let recovery = secrets.iter().flat_map(|s| s.to_repr()).collect::<Vec<_>>();
    let encrypted = encrypt(private_pem, &recovery)?;
    let mut output = vec![Vec::with_capacity(4); shares as usize];
    for secret in secrets {
        let result = vsss_rs::pedersen::split_secret::<P256Share, P256Verifier>(
            threshold as usize,
            shares as usize,
            &IdentifierPrimeField(secret),
            None,
            None,
            None,
            &mut rng,
        )
        .map_err(vsss_error)?;
        for i in 0..shares as usize {
            output[i].push(VerifiableComponent{
                value:scalar_hex(&result.secret_shares()[i].value().0), blinder:scalar_hex(&result.blinder_shares()[i].value().0),
                feldman_generator:point_hex(&<VerifierSet as FeldmanVerifierSet<P256Share,P256Verifier>>::generator(result.feldman_verifier_set()).0),
                feldman_commitments:<VerifierSet as FeldmanVerifierSet<P256Share,P256Verifier>>::verifiers(result.feldman_verifier_set()).iter().map(|p|point_hex(&p.0)).collect(),
                pedersen_secret_generator:point_hex(&<VerifierSet as PedersenVerifierSet<P256Share,P256Verifier>>::secret_generator(result.pedersen_verifier_set()).0),
                pedersen_blinder_generator:point_hex(&<VerifierSet as PedersenVerifierSet<P256Share,P256Verifier>>::blinder_generator(result.pedersen_verifier_set()).0),
                pedersen_commitments:<VerifierSet as PedersenVerifierSet<P256Share,P256Verifier>>::blind_verifiers(result.pedersen_verifier_set()).iter().map(|p|point_hex(&p.0)).collect(),
            });
        }
    }
    let shamir = ShamirParameters {
        shares,
        threshold,
        field: "P-256 scalar field".into(),
        components: 4,
    };
    Ok(output
        .into_iter()
        .enumerate()
        .map(|(i, components)| ShamirShareFile {
            version: ShareFormatVersion::V1,
            public_key: public_key.clone(),
            shamir: shamir.clone(),
            x: (i + 1) as u8,
            encrypted_private_key: encrypted.clone(),
            components,
        })
        .collect())
}

/// Verify both commitment sets, reconstruct, and decrypt the exact PEM bytes.
pub fn recover_private_key(files: &[ShamirShareFile]) -> Result<Vec<u8>, Box<dyn Error>> {
    let first = files.first().ok_or("no shares supplied")?;
    if files.len() < first.shamir.threshold as usize {
        return Err("not enough shares".into());
    }
    for file in files {
        file.verify()?;
    }
    let mut recovery = Vec::with_capacity(128);
    for ci in 0..4 {
        let mut shares = Vec::new();
        for file in files {
            if file.public_key != first.public_key
                || file.shamir != first.shamir
                || file.encrypted_private_key != first.encrypted_private_key
            {
                return Err("shares do not describe the same recovery set".into());
            }
            shares.push(make_share(file.x, &file.components[ci].value)?);
        }
        recovery.extend_from_slice(shares.combine().map_err(vsss_error)?.0.to_repr().as_ref());
    }
    decrypt(&first.encrypted_private_key, &recovery)
}

impl ShamirShareFile {
    /// Verify this share against both serialized commitment sets.
    pub fn verify(&self) -> Result<(), Box<dyn Error>> {
        if self.x == 0 || self.components.len() != self.shamir.components as usize {
            return Err("share has an invalid number or component count".into());
        }
        for c in &self.components {
            let share = make_share(self.x, &c.value)?;
            let blind = make_share(self.x, &c.blinder)?;
            let pc = c
                .pedersen_commitments
                .iter()
                .map(|p| point(p).map(ValueGroup))
                .collect::<Result<Vec<_>, _>>()?;
            let ps=<VerifierSet as PedersenVerifierSet<P256Share,P256Verifier>>::pedersen_set_with_generators_and_verifiers(ValueGroup(point(&c.pedersen_secret_generator)?),ValueGroup(point(&c.pedersen_blinder_generator)?),&pc);
            ps.verify_share_and_blinder(&share, &blind)
                .map_err(vsss_error)?;
            let fc = c
                .feldman_commitments
                .iter()
                .map(|p| point(p).map(ValueGroup))
                .collect::<Result<Vec<_>, _>>()?;
            let fs=<VerifierSet as FeldmanVerifierSet<P256Share,P256Verifier>>::feldman_set_with_generator_and_verifiers(ValueGroup(point(&c.feldman_generator)?),&fc);
            fs.verify_share(&share).map_err(vsss_error)?;
        }
        Ok(())
    }
}

fn encrypt(plain: &[u8], secret: &[u8]) -> Result<EncryptedPrivateKey, Box<dyn Error>> {
    let key = openssl::hash::hash(MessageDigest::sha256(), secret)?;
    let mut nonce = [0; 12];
    openssl::rand::rand_bytes(&mut nonce)?;
    let mut c = Crypter::new(Cipher::aes_256_gcm(), Mode::Encrypt, &key, Some(&nonce))?;
    let mut out = vec![0; plain.len() + 16];
    let mut n = c.update(plain, &mut out)?;
    n += c.finalize(&mut out[n..])?;
    out.truncate(n);
    let mut tag = [0; 16];
    c.get_tag(&mut tag)?;
    Ok(EncryptedPrivateKey {
        cipher: "AES-256-GCM".into(),
        key_derivation: "SHA-256 of the 128-byte recovery secret".into(),
        nonce: hex(&nonce),
        ciphertext: hex(&out),
        tag: hex(&tag),
    })
}
fn decrypt(e: &EncryptedPrivateKey, secret: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let key = openssl::hash::hash(MessageDigest::sha256(), secret)?;
    let nonce = unhex(&e.nonce)?;
    let tag = unhex(&e.tag)?;
    let data = unhex(&e.ciphertext)?;
    let mut c = Crypter::new(Cipher::aes_256_gcm(), Mode::Decrypt, &key, Some(&nonce))?;
    c.set_tag(&tag)?;
    let mut out = vec![0; data.len() + 16];
    let mut n = c.update(&data, &mut out)?;
    n += c.finalize(&mut out[n..])?;
    out.truncate(n);
    Ok(out)
}
fn make_share(x: u8, v: &str) -> Result<P256Share, Box<dyn Error>> {
    Ok(P256Share::with_identifier_and_value(
        IdentifierPrimeField(Scalar::from(x as u64)),
        IdentifierPrimeField(scalar(v)?),
    ))
}
fn scalar(s: &str) -> Result<Scalar, Box<dyn Error>> {
    let a: [u8; 32] = unhex(s)?
        .try_into()
        .map_err(|_| "P-256 scalar must be 32 bytes")?;
    Option::from(Scalar::from_repr(a.into())).ok_or_else(|| "invalid P-256 scalar".into())
}
fn point(s: &str) -> Result<ProjectivePoint, Box<dyn Error>> {
    let b = unhex(s)?;
    Option::from(ProjectivePoint::from_bytes(b.as_slice().into()))
        .ok_or_else(|| "invalid compressed P-256 point".into())
}
fn scalar_hex(s: &Scalar) -> String {
    hex(s.to_repr().as_ref())
}
fn point_hex(p: &ProjectivePoint) -> String {
    hex(p.to_bytes().as_ref())
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn unhex(s: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.into()))
        .collect()
}
fn vsss_error(e: vsss_rs::Error) -> Box<dyn Error> {
    format!("verifiable secret sharing failed: {e:?}").into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_round_trip_and_yaml() {
        let f = MultiHash::new(crate::multihash::HashAlgorithm::SHA256, "ab".repeat(32));
        let s = split_private_key(b"exact\0private PEM bytes\n", f, 5, 3).unwrap();
        let y = yaml_serde::to_string(&s[0]).unwrap();
        assert!(y.contains("pedersen_commitments:"));
        assert_eq!(
            recover_private_key(&s).unwrap(),
            b"exact\0private PEM bytes\n"
        );
    }

    #[test]
    fn pedersen_commitment_rejects_a_modified_share() {
        let f = MultiHash::new(crate::multihash::HashAlgorithm::SHA256, "cd".repeat(32));
        let mut shares = split_private_key(b"private PEM", f, 3, 2).unwrap();
        shares[0].components[0].value = "01".repeat(32);
        assert!(recover_private_key(&shares).is_err());
    }
}
