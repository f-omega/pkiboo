use super::backend::Media;
use crate::multihash::MultiHash;
use crate::pkiboo::Key;
use crate::util::Name;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

/// A manifest is something that lists all files on this media.
#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub(crate) files: Vec<SignedFile>,
}

impl Manifest {
    fn empty() -> Self {
        Manifest { files: Vec::new() }
    }

    fn lookup_file(&self, path: &PathBuf) -> Option<&SignedFile> {
        self.files.iter().find(|l| &l.path == path)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct SignedFile {
    /// Path relative to the PKI directory
    pub(crate) path: PathBuf,

    /// Hash of this file
    hash: MultiHash,

    /// Signature of this hash against the private key it's testifying to
    signature: crate::keypair::Signature,

    /// Name of the key this is testified to
    pub(crate) key: Name<Key>,
}

impl SignedFile {
    fn nonce(path: &PathBuf, hash: String) -> String {
        let mut nonce = hash.to_string();
        nonce.push('\0');
        nonce.push_str(&path.to_string_lossy().to_string());
        nonce
    }

    fn sign(
        path: PathBuf,
        key: &crate::keypair::LoadedKey,
        contents: &secrecy::SecretBox<Vec<u8>>,
    ) -> Result<SignedFile, Box<dyn Error>> {
        let hash = MultiHash::with_default_algo(contents.expose_secret());
        let nonce = Self::nonce(&path, hash.to_string());
        let signature = key
            .pkey
            .sign(secrecy::SecretBox::new(Box::new(nonce.into())))?;
        Ok(SignedFile {
            path,
            hash,
            signature,
            key: key.key.name.clone(),
        })
    }

    fn verifies(
        &self,
        key: &openssl::pkey::PKey<openssl::pkey::Public>,
    ) -> Result<bool, Box<dyn Error>> {
        let nonce = Self::nonce(&self.path, self.hash.to_string());
        self.signature.verify(key, nonce.as_bytes())
    }
}

/// A manifest that is being modified from a particular media
pub struct OpenManifest {
    pub(crate) media: Arc<dyn Media>,
    pub(crate) current: Manifest,
}

/// An error encountered while opening an existing media manifest.
///
/// Keeping these cases distinct lets assessment report damage to the medium
/// without parsing the manifest or reaching through to the storage backend
/// itself.
#[derive(Debug)]
pub enum OpenManifestError {
    Missing,
    Read(Box<dyn Error>),
    Invalid(Box<dyn Error>),
}

impl fmt::Display for OpenManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "media manifest is missing"),
            Self::Read(error) => write!(f, "could not read media manifest: {error}"),
            Self::Invalid(error) => write!(f, "media manifest is invalid: {error}"),
        }
    }
}

impl Error for OpenManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Missing => None,
            Self::Read(error) | Self::Invalid(error) => Some(error.as_ref()),
        }
    }
}

impl std::ops::Deref for OpenManifest {
    type Target = Manifest;

    fn deref(&self) -> &Self::Target {
        &self.current
    }
}

impl std::ops::DerefMut for OpenManifest {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.current
    }
}

static MANIFEST_KEY: &'static str = "manifest.yaml";
impl OpenManifest {
    /// Open the manifest already stored on a medium.
    ///
    /// A missing manifest is an error. Call [`OpenManifest::create`] only when
    /// initializing a new medium.
    pub async fn new(media: Arc<dyn Media>) -> Result<Self, OpenManifestError> {
        match media.get(&MANIFEST_KEY.into()).await {
            Ok(Some(contents)) => {
                let current = yaml_serde::from_slice(&contents)
                    .map_err(|error| OpenManifestError::Invalid(error.into()))?;
                Ok(OpenManifest {
                    media: media.clone(),
                    current,
                })
            }
            Ok(None) => Err(OpenManifestError::Missing),
            Err(error) => Err(OpenManifestError::Read(error)),
        }
    }

    /// Create an empty in-memory manifest for a newly initialized medium.
    pub fn create(media: Arc<dyn Media>) -> Self {
        Self {
            media,
            current: Manifest::empty(),
        }
    }

    pub async fn save(&self) -> Result<(), Box<dyn Error>> {
        let s = yaml_serde::to_string(&self.current)?;
        self.media
            .put(&MANIFEST_KEY.into(), &s.into_bytes())
            .await?;
        Ok(())
    }

    /// Read a manifest file and verify it against keys stored in the local database.
    ///
    /// Returns the bytes in a secret box or None if the file could not be found.
    ///
    /// Otherwise returns an error.
    #[allow(dead_code)]
    pub async fn read_verified(
        &self,
        db: &crate::pkiboo::Db,
        path: &PathBuf,
    ) -> Result<Option<secrecy::SecretBox<Vec<u8>>>, Box<dyn Error>> {
        if let Some(sfile) = self.current.lookup_file(path) {
            // Read the file from media
            let data = secrecy::SecretBox::new(
                Box::new(
                    self.media.get(&path.to_string_lossy().to_string()).await?.ok_or::<String>(format!("File {} was found in the manifest but not present, run 'media repair' to attempt to repair this media", path.display()).into())?
                ));

            let key = db.lookup_key(&sfile.key).ok_or::<String>(
                format!(
                    "File was signed by key {} which could not be found",
                    sfile.key
                )
                .into(),
            )?;
            let pkey = key.load_public_key()?;

            let actual_hash = MultiHash::hash(sfile.hash.kind.clone(), data.expose_secret());
            if actual_hash != sfile.hash {
                return Err(format!(
                    "File contents do not match. Got hash {}, expected {}",
                    actual_hash, sfile.hash
                )
                .into());
            }

            if sfile.verifies(&pkey)? {
                Ok(Some(data))
            } else {
                Err(format!("The signature for {} could not be verified", path.display()).into())
            }
        } else {
            Ok(None) // Not found in our manifest
        }
    }

    /// Write a file into the manifest. The file is written immediately, but the manifest is not.
    ///
    /// This will fail if the file already exists in the manifest and the contents
    /// do not hash to the same thing
    pub async fn write_file(
        &mut self,
        file: PathBuf,
        key: &crate::keypair::LoadedKey,
        contents: secrecy::SecretBox<Vec<u8>>,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(existing) = self.current.lookup_file(&file) {
            if !existing.hash.check(contents.expose_secret()) {
                return Err(format!(
                    "File {} already exists and its contents do not match",
                    file.display()
                )
                .into());
            }
            Ok(())
        } else {
            let sfile = SignedFile::sign(file.clone(), &key, &contents)?;
            self.current.files.push(sfile);

            self.media
                .put(
                    &file.to_string_lossy().to_string(),
                    contents.expose_secret(),
                )
                .await?;

            Ok(())
        }
    }
}
