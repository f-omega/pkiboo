use super::backend::Media as MediaBackend;
use super::manifest::{ManifestFileError, OpenManifest, OpenManifestError};
use crate::pkiboo::{Db, Media};
use crate::util::Name;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// A problem found while comparing a medium's manifest, signed contents, and
/// the contents expected by the current Pkiboo database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MediaIssue {
    MissingManifest,
    UnreadableManifest {
        message: String,
    },
    UnsafePath {
        path: PathBuf,
    },
    DuplicateManifestEntry {
        path: PathBuf,
    },
    UnreadableFile {
        path: PathBuf,
        message: String,
    },
    MissingFile {
        path: PathBuf,
    },
    UnknownSigningKey {
        path: PathBuf,
        key: String,
    },
    InvalidSigningKey {
        path: PathBuf,
        key: String,
        message: String,
    },
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    SignatureCheckFailed {
        path: PathBuf,
        key: String,
        message: String,
    },
    InvalidSignature {
        path: PathBuf,
        key: String,
    },
    MissingExpectedEntry {
        path: PathBuf,
        kind: String,
        name: String,
    },
    UnexpectedManifestEntry {
        path: PathBuf,
    },
}

/// Successful evidence retained alongside issues. Consumers such as
/// `media verify` can use this to update per-copy verification timestamps only
/// for material that actually passed every manifest check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedMediaFile {
    pub path: PathBuf,
    pub signing_key: String,
}

/// Complete read-only assessment of one medium at a point in time.
///
/// This is deliberately independent of UI and repair policy. Verification can
/// present it as-is; repair can later translate the same issues into an
/// explicit plan without repeating or subtly changing validation logic.
pub struct MediaAssessment {
    pub media: Name<Media>,
    pub checked_at: chrono::DateTime<chrono::Utc>,
    pub issues: Vec<MediaIssue>,
    pub verified_files: Vec<VerifiedMediaFile>,
}

impl MediaAssessment {
    pub fn is_healthy(&self) -> bool {
        self.issues.is_empty()
    }

    /// Collect every independently detectable issue instead of failing at the
    /// first corrupt or missing file. Errors returned from this method are
    /// operational failures that prevent assessment itself; damage found on
    /// the medium is represented in `issues`.
    pub async fn collect(
        db: &Db,
        media_record: &Media,
        backend: Arc<dyn MediaBackend>,
    ) -> Result<Self, Box<dyn Error>> {
        let mut assessment = Self {
            media: media_record.label.clone(),
            checked_at: chrono::Utc::now(),
            issues: Vec::new(),
            verified_files: Vec::new(),
        };

        // Complete key copies are the only secret artifact currently written
        // by implemented commands. Numbered split paths will join this map
        // when split creation defines their on-media representation.
        let expected = db
            .keys
            .iter()
            .filter(|key| key.backups.contains(&media_record.label))
            .map(|key| {
                (
                    key.key_path(),
                    ("private key".to_owned(), key.name.to_string()),
                )
            })
            .collect::<HashMap<_, _>>();

        let open_manifest = match OpenManifest::new(backend).await {
            Ok(manifest) => manifest,
            Err(OpenManifestError::Missing) => {
                assessment.issues.push(MediaIssue::MissingManifest);
                add_all_expected_as_missing(&mut assessment, &expected);
                return Ok(assessment);
            }
            Err(OpenManifestError::Read(error) | OpenManifestError::Invalid(error)) => {
                assessment.issues.push(MediaIssue::UnreadableManifest {
                    message: error.to_string(),
                });
                add_all_expected_as_missing(&mut assessment, &expected);
                return Ok(assessment);
            }
        };

        let mut seen = HashSet::new();

        for entry in &open_manifest.current.files {
            let path = entry.path.clone();

            if !safe_relative_path(&path) {
                assessment.issues.push(MediaIssue::UnsafePath { path });
                continue;
            }

            if !seen.insert(path.clone()) {
                assessment
                    .issues
                    .push(MediaIssue::DuplicateManifestEntry { path });
                continue;
            }

            if !expected.contains_key(&path) {
                assessment
                    .issues
                    .push(MediaIssue::UnexpectedManifestEntry { path: path.clone() });
            }

            match open_manifest.read_verified(db, &path).await {
                Ok(Some(_)) => assessment.verified_files.push(VerifiedMediaFile {
                    path,
                    signing_key: entry.key.to_string(),
                }),
                Ok(None) => assessment.issues.push(MediaIssue::MissingFile { path }),
                Err(error) => assessment.issues.push(issue_for_file_error(path, error)),
            }
        }

        for (path, (kind, name)) in expected {
            if !seen.contains(&path) {
                assessment
                    .issues
                    .push(MediaIssue::MissingExpectedEntry { path, kind, name });
            }
        }

        Ok(assessment)
    }
}

fn issue_for_file_error(path: PathBuf, error: ManifestFileError) -> MediaIssue {
    match error {
        ManifestFileError::Read(error) => MediaIssue::UnreadableFile {
            path,
            message: error.to_string(),
        },
        ManifestFileError::Missing => MediaIssue::MissingFile { path },
        ManifestFileError::UnknownSigningKey(key) => MediaIssue::UnknownSigningKey {
            path,
            key: key.to_string(),
        },
        ManifestFileError::InvalidPublicKey { key, source } => MediaIssue::InvalidSigningKey {
            path,
            key: key.to_string(),
            message: source.to_string(),
        },
        ManifestFileError::HashMismatch { expected, actual } => MediaIssue::HashMismatch {
            path,
            expected,
            actual,
        },
        ManifestFileError::SignatureCheck { key, source } => {
            MediaIssue::SignatureCheckFailed {
                path,
                key: key.to_string(),
                message: source.to_string(),
            }
        }
        ManifestFileError::InvalidSignature(key) => MediaIssue::InvalidSignature {
            path,
            key: key.to_string(),
        },
    }
}

fn add_all_expected_as_missing(
    assessment: &mut MediaAssessment,
    expected: &HashMap<PathBuf, (String, String)>,
) {
    assessment.issues.extend(expected.iter().map(
        |(path, (kind, name))| MediaIssue::MissingExpectedEntry {
            path: path.clone(),
            kind: kind.clone(),
            name: name.clone(),
        },
    ));
}

fn safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::safe_relative_path;
    use std::path::Path;

    #[test]
    fn manifest_paths_must_stay_inside_media_root() {
        assert!(safe_relative_path(Path::new("keys/example/private.pem")));
        assert!(!safe_relative_path(Path::new("../private.pem")));
        assert!(!safe_relative_path(Path::new("/private.pem")));
        assert!(!safe_relative_path(Path::new("")));
    }
}
