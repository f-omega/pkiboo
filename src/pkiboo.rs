use crate::cli_common::CliBackend;
use crate::ui::{ListView, Ui};
use crate::util::Name;
use itertools::Itertools;
use resolve_path::PathResolveExt;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::cmp::Ordering;
use std::io::IsTerminal;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::{collections::HashMap, error::Error, path::PathBuf, sync::Arc};

pub struct PkiBoo<UiBackend> {
    db_path: PathBuf,
    ui_backend: UiBackend,
}

impl<UiBackend: Ui> PkiBoo<UiBackend> {
    pub fn ui(&self) -> &UiBackend {
        &self.ui_backend
    }

    pub fn open_database(&self) -> Result<OpenedDb, Box<dyn Error>> {
        match std::fs::read_to_string(&self.db_path) {
            Ok(contents) => {
                let db = yaml_serde::from_str(&contents)?;
                Ok(OpenedDb {
                    db,
                    db_path: self.db_path.clone(),
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(OpenedDb {
                db: Db::empty(),
                db_path: self.db_path.clone(),
            }),
            Err(e) => Err(Box::new(e)),
        }
    }
}

impl PkiBoo<CliBackend> {
    pub fn from_cli_opts(options: &crate::CliOptions) -> PkiBoo<CliBackend> {
        let path = options
            .db_path
            .clone()
            .unwrap_or("~/.pkiboo/db.yaml".into());
        let db_path = std::path::Path::new(&path).resolve().to_path_buf();

        if std::io::stdin().is_terminal() {
            let (ready_tx, ready_rx) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                use std::process::Stdio;
                use tokio::process::Command;

                let pid = std::process::id();

                let (mut reader, writer) = os_pipe::pipe()?;
                let bfd = writer.as_raw_fd();

                let mut agent = Command::new("pkttyagent");
                agent
                    .arg("--process")
                    .arg(pid.to_string())
                    .arg("--notify-fd")
                    .arg("3")
                    .arg("--fallback")
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .kill_on_drop(true);
                unsafe {
                    agent.pre_exec(move || {
                        if libc::dup2(bfd, 3) == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    })
                };
                let mut agent = agent.spawn()?;
                drop(writer);

                let mut buf: [u8; 1] = [0u8];
                let _ = reader.read_exact(&mut buf); // Doesn't matter how it ends, but whichever way it does, we were notified

                ready_tx.send_replace(true);

                agent.wait().await
            });
            PkiBoo {
                db_path,
                ui_backend: CliBackend::new(ready_rx),
            }
        } else {
            panic!("Can't run non-interactively yet");
        }
    }
}

/// The database of everything we know about
#[derive(Serialize, Deserialize, Clone)]
pub struct Db {
    /// Keypairs
    pub keys: Vec<Key>,

    /// Certificates
    pub certs: Vec<Cert>,

    /// Splits
    pub splits: Vec<Split>,

    /// Backend media
    pub media: Vec<Media>,
}

static DB_KEY: &'static str = "db.yaml";

impl Db {
    fn empty() -> Self {
        Db {
            keys: Vec::new(),
            certs: Vec::new(),
            splits: Vec::new(),
            media: Vec::new(),
        }
    }

    pub fn lookup_media(&self, nm: &Name<Media>) -> Option<&Media> {
        self.media.iter().find(|n| &n.label == nm)
    }

    pub fn lookup_media_by_id(&self, id: &crate::media::MediaId) -> Option<&Media> {
        self.media.iter().find(|n| &n.id == id)
    }

    pub fn lookup_key(&self, nm: &Name<Key>) -> Option<&Key> {
        self.keys.iter().find(|n| n.name == *nm)
    }

    pub fn lookup_key_by_public_key(
        &self,
        pkey: &openssl::pkey::PKey<openssl::pkey::Public>,
    ) -> Option<&Key> {
        pkey.public_key_to_pem()
            .ok()
            .and_then(|pem| String::from_utf8(pem).ok())
            .and_then(|pem| self.keys.iter().find(|n| n.public_key == pem))
    }

    pub fn lookup_cert(&self, nm: &Name<Cert>) -> Option<&Cert> {
        self.certs.iter().find(|n| n.name == *nm)
    }

    pub fn lookup_split(&self, nm: &Name<Split>) -> Option<&Split> {
        self.splits.iter().find(|n| n.label == *nm)
    }

    /// Write a non-authoritative snapshot that may help reconstruct local
    /// state after loss. The local database remains authoritative during
    /// ordinary operation; snapshots on different media may be stale.
    pub async fn write_recovery_hint(
        &self,
        media: Arc<dyn crate::media::backend::Media>,
    ) -> Result<(), Box<dyn Error>> {
        let s = yaml_serde::to_string(self)?;
        media.put(&DB_KEY.into(), &s.into_bytes()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod verification_tests {
    use super::*;

    fn placement(share: u32, media: &str) -> SplitBackup {
        SplitBackup {
            share: ShareNumber(share),
            media: Name::new(media.into()),
        }
    }

    fn split(verifications: Vec<SplitVerification>) -> Split {
        Split {
            label: Name::new("test split".into()),
            key: Name::new("test key".into()),
            num_splits: 5,
            min_splits: 3,
            meta: Meta::new(),
            backups: (1..=5)
                .map(|share| placement(share, &format!("media {share}")))
                .collect(),
            verifications,
        }
    }

    #[test]
    fn threshold_number_of_fresh_shares_is_degraded() {
        let now = chrono::Utc::now();
        let split = split(vec![SplitVerification {
            verified_at: now,
            shares: vec![
                placement(1, "media 1"),
                placement(2, "media 2"),
                placement(3, "media 3"),
            ],
        }]);

        assert_eq!(
            split.verification_status_at(now),
            PrivateEntityVerification::Degraded {
                verified: 3,
                expected: 5
            }
        );
    }

    #[test]
    fn all_shares_can_be_verified_across_fresh_sessions() {
        let now = chrono::Utc::now();
        let split = split(vec![
            SplitVerification {
                verified_at: now - chrono::Duration::days(10),
                shares: vec![placement(1, "media 1"), placement(2, "media 2")],
            },
            SplitVerification {
                verified_at: now,
                shares: vec![
                    placement(3, "media 3"),
                    placement(4, "media 4"),
                    placement(5, "media 5"),
                ],
            },
        ]);

        assert_eq!(
            split.verification_status_at(now),
            PrivateEntityVerification::Complete
        );
    }

    #[test]
    fn replicas_of_one_share_only_count_once() {
        let now = chrono::Utc::now();
        let split = split(vec![SplitVerification {
            verified_at: now,
            shares: vec![
                placement(1, "media 1"),
                placement(1, "other media"),
                placement(2, "media 2"),
            ],
        }]);

        assert_eq!(
            split.verification_status_at(now),
            PrivateEntityVerification::NotVerified
        );
    }
}

pub struct OpenedDb {
    db: Db,
    db_path: PathBuf,
}

impl std::ops::Deref for OpenedDb {
    type Target = Db;

    fn deref(&self) -> &Self::Target {
        &self.db
    }
}

impl OpenedDb {
    pub fn transaction<'a>(&'a mut self) -> DbTx<'a> {
        let copy = self.db.clone();
        DbTx {
            db: self,
            copy,
            failed: false,
        }
    }

    fn write(&self) -> Result<(), Box<dyn Error>> {
        let tmp_path = self.db_path.with_extension("tmp");
        let contents = yaml_serde::to_string(&self.db)?;
        std::fs::create_dir_all(self.db_path.parent().unwrap())?;
        std::fs::write(&tmp_path, &contents)?;
        std::fs::rename(&tmp_path, &self.db_path)?;
        Ok(())
    }
}

pub struct DbTx<'a> {
    db: &'a mut OpenedDb,
    copy: Db,
    failed: bool,
}

impl<'a> std::ops::Deref for DbTx<'a> {
    type Target = Db;

    fn deref(&self) -> &Self::Target {
        &self.copy
    }
}

impl<'a> std::ops::DerefMut for DbTx<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.copy
    }
}

impl<'a> std::ops::Drop for DbTx<'a> {
    fn drop(&mut self) {
        if !self.failed {
            std::mem::swap(&mut self.db.db, &mut self.copy);
            self.db.write().unwrap();
        }
    }
}

impl<'a> DbTx<'a> {
    pub fn add_key(&mut self, key: Key) {
        self.keys.push(key)
    }

    pub fn add_cert(&mut self, cert: Cert) {
        self.certs.push(cert)
    }

    pub fn add_media(&mut self, m: Media) {
        self.media.push(m)
    }

    /// Remove a medium from inventory and from every private entity's backup
    /// and verification state. Safety policy is checked by the caller before
    /// this low-level mutation is performed.
    pub fn forget_media(&mut self, media: &Name<Media>) {
        self.media.retain(|candidate| &candidate.label != media);

        for key in &mut self.keys {
            key.backups.retain(|backup| backup != media);
            key.verifications
                .retain(|verification| &verification.media != media);
        }

        for split in &mut self.splits {
            split.backups.retain(|backup| &backup.media != media);
            for verification in &mut split.verifications {
                verification.shares.retain(|share| &share.media != media);
            }
            split
                .verifications
                .retain(|verification| !verification.shares.is_empty());
        }
    }

    pub fn update_cert(&mut self, mut cert: Cert) -> Result<Cert, Box<dyn Error>> {
        match self
            .certs
            .iter()
            .enumerate()
            .find(|(_, n)| n.name == cert.name)
        {
            None => Err(format!("Certificate {} does not exist", cert.name).into()),
            Some((i, _)) => {
                std::mem::swap(&mut self.certs[i], &mut cert);
                Ok(cert)
            }
        }
    }

    pub fn update_split(&mut self, mut split: Split) -> Result<Split, Box<dyn Error>> {
        match self
            .splits
            .iter()
            .enumerate()
            .find(|(_, n)| n.label == split.label)
        {
            None => Err(format!("Split {} does not exist", split.label).into()),
            Some((i, _)) => {
                std::mem::swap(&mut self.splits[i], &mut split);
                Ok(split)
            }
        }
    }

    pub fn update_media(&mut self, mut m: Media) -> Result<Media, Box<dyn Error>> {
        match self.media.iter().enumerate().find(|(_, n)| n.id == m.id) {
            None => Err(format!("{} does not exist", m.id).into()),
            Some((i, _)) => {
                std::mem::swap(&mut self.media[i], &mut m);
                Ok(m)
            }
        }
    }

    pub fn update_key(&mut self, mut k: Key) -> Result<Key, Box<dyn Error>> {
        match self.keys.iter().enumerate().find(|(_, n)| n.name == k.name) {
            None => Err(format!("Key {} does not exist", k.name).into()),
            Some((i, _)) => {
                std::mem::swap(&mut self.keys[i], &mut k);
                Ok(k)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Key {
    pub name: Name<Self>,
    pub algorithm: crate::keypair::Algorithm,

    /// PEM-encoded public key
    pub public_key: String,

    pub meta: Meta,

    pub backups: Vec<Name<Media>>,

    /// Most recent successful verification of each complete copy.
    #[serde(default)]
    pub verifications: Vec<KeyCopyVerification>,
}

impl Key {
    pub fn new(name: Name<Self>, algorithm: crate::keypair::Algorithm, public_key: String) -> Self {
        Key {
            name,
            algorithm,
            meta: Meta::new(),
            backups: Vec::new(),
            verifications: Vec::new(),
            public_key,
        }
    }

    pub fn add_backup(&mut self, media: Name<Media>) {
        if !self.backups.contains(&media) {
            self.backups.push(media)
        }
    }

    pub fn remove_backup(&mut self, media: &Name<Media>) {
        self.backups.retain(|backup| backup != media);
        self.clear_verification(media);
    }

    pub fn record_verification(
        &mut self,
        media: Name<Media>,
        verified_at: chrono::DateTime<chrono::Utc>,
    ) {
        record_verification(&mut self.verifications, media, verified_at);
    }

    /// Remove stale positive evidence after a copy fails verification.
    pub fn clear_verification(&mut self, media: &Name<Media>) {
        self.verifications
            .retain(|verification| &verification.media != media);
    }

    pub fn key_path(&self) -> PathBuf {
        PathBuf::new()
            .join("keys")
            .join(self.name.to_string())
            .join("private.pem")
    }

    pub fn load_public_key(
        &self,
    ) -> Result<openssl::pkey::PKey<openssl::pkey::Public>, Box<dyn Error>> {
        Ok(openssl::pkey::PKey::public_key_from_pem(
            self.public_key.as_bytes(),
        )?)
    }
}

impl crate::ui::ListItem for Key {
    fn column_names() -> &'static [&'static str] {
        &["name", "algorithm", "backups"]
    }

    fn get_field(&self, col: usize) -> String {
        match col {
            0 => self.name.clone().into(),
            1 => format!("{}", self.algorithm).into(),
            2 => self.backups.iter().cloned().join(","),
            _ => "".into(),
        }
    }
}

/// A certificate managed by pkiboo
#[derive(Serialize, Deserialize, Clone)]
pub struct Cert {
    pub name: Name<Self>,
    pub key: Name<Key>,

    /// Issuing certificate, or none when this certificate is self-signed.
    pub issuer: Option<Name<Self>>,

    /// PEM-encoded public certificate.
    pub certificate: String,

    pub created_on: chrono::DateTime<chrono::Utc>,

    /// Once set, this certificate may never be used for new issuance. The
    /// public certificate remains stored for history and chain validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<chrono::DateTime<chrono::Utc>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement_reason: Option<String>,

    pub meta: Meta,
}

impl Cert {
    /// Whether this certificate is eligible to issue another certificate now.
    /// Retirement is permanent; ordinary X.509 validity is checked as well so
    /// callers cannot accidentally issue with an expired or not-yet-valid CA.
    pub fn is_valid_issuer(&self) -> Result<bool, Box<dyn Error>> {
        if self.retired_at.is_some() {
            return Ok(false);
        }

        let certificate = openssl::x509::X509::from_pem(self.certificate.as_bytes())?;
        let now = openssl::asn1::Asn1Time::days_from_now(0)?;
        Ok(certificate.not_before().compare(&now)?.is_le()
            && certificate.not_after().compare(&now)?.is_ge())
    }

    /// Classify certificate lifetime for health reporting. Retirement is a
    /// deliberate terminal state, not a certificate-health failure.
    pub fn validity_at(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<CertificateValidity, Box<dyn Error>> {
        if self.retired_at.is_some() {
            return Ok(CertificateValidity::Retired);
        }

        let certificate = openssl::x509::X509::from_pem(self.certificate.as_bytes())?;
        let openssl_now = openssl::asn1::Asn1Time::from_unix(now.timestamp())?;
        let expiry_warning = openssl::asn1::Asn1Time::from_unix(
            (now + chrono::Duration::days(CERT_EXPIRY_WARNING_DAYS)).timestamp(),
        )?;

        if certificate.not_before().compare(&openssl_now)? == Ordering::Greater {
            Ok(CertificateValidity::NotYetValid)
        } else if certificate.not_after().compare(&openssl_now)? == Ordering::Less {
            Ok(CertificateValidity::Expired)
        } else if certificate.not_after().compare(&expiry_warning)? != Ordering::Greater {
            Ok(CertificateValidity::ExpiringSoon)
        } else {
            Ok(CertificateValidity::Current)
        }
    }
}

impl crate::ui::ListItem for Cert {
    fn column_names() -> &'static [&'static str] {
        &["name", "key", "issuer", "created", "retired"]
    }

    fn get_field(&self, col: usize) -> String {
        match col {
            0 => self.name.to_string(),
            1 => self.key.to_string(),
            2 => self
                .issuer
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            3 => self.created_on.to_rfc3339(),
            4 => self
                .retired_at
                .map(|retired_at| retired_at.to_rfc3339())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}

/// Registered media
#[derive(Serialize, Deserialize, Clone)]
pub struct Media {
    pub label: Name<Self>,
    pub id: crate::media::MediaId,

    /// Trusted for backup storage (if false, only pieces are allowed)
    pub trusted: bool,

    pub meta: Meta,
}

impl Media {
    pub fn new(label: Name<Self>, id: crate::media::MediaId, trusted: bool) -> Self {
        Self {
            label,
            id,
            trusted,
            meta: Meta::new(),
        }
    }
}

impl crate::ui::ListItem for Media {
    fn column_names() -> &'static [&'static str] {
        &["label", "id", "trusted"]
    }

    fn get_field(&self, col: usize) -> String {
        match col {
            0 => self.label.clone().into(),
            1 => format!("{}", self.id).into(),
            2 => format!("{}", self.trusted).into(), // TODO typed columns
            _ => "".into(),
        }
    }
}

/// Split of a key
#[derive(Serialize, Deserialize, Clone)]
pub struct Split {
    pub label: Name<Self>,

    /// The key that was split
    pub key: Name<Key>,

    pub num_splits: u32,
    pub min_splits: u32,

    pub meta: Meta,

    pub backups: Vec<SplitBackup>,

    /// Most recent successful verification of each stored share.
    #[serde(default)]
    pub verifications: Vec<SplitVerification>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateEntityVerification {
    Complete,
    Degraded { verified: usize, expected: usize },
    NotVerified,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeyCopyVerification {
    pub media: Name<Media>,
    pub verified_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ShareNumber(pub u32);

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct SplitBackup {
    pub share: ShareNumber,
    pub media: Name<Media>,
}

/// A split verification exists only when reconstruction with the presented
/// numbered shares succeeded. Health is derived from the union of successful
/// verification evidence that remains inside the freshness interval.
#[derive(Serialize, Deserialize, Clone)]
pub struct SplitVerification {
    pub verified_at: chrono::DateTime<chrono::Utc>,
    pub shares: Vec<SplitBackup>,
}

/// Successful verification remains current for one quarter. This is a policy
/// constant rather than stored state: changing the policy immediately
/// reevaluates every private copy from its durable verification timestamp.
pub const VERIFICATION_MAX_AGE_DAYS: i64 = 90;

/// A key should retain at least two independent complete copies.
pub const MIN_KEY_REPLICAS: usize = 2;

const CERT_EXPIRY_WARNING_DAYS: i64 = 30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CertificateValidity {
    Retired,
    NotYetValid,
    Expired,
    ExpiringSoon,
    Current,
}

/// Severity of a durable-state health problem found in the database.
pub enum HealthSeverity {
    Warning,
    Critical,
}

impl std::fmt::Display for HealthSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Warning => write!(f, "warning"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// One actionable health problem derived from Pkiboo's stored state.
pub struct HealthIssue {
    pub severity: HealthSeverity,
    pub entity: String,
    pub detail: String,
}

impl HealthIssue {
    fn warning(entity: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            severity: HealthSeverity::Warning,
            entity: entity.into(),
            detail: detail.into(),
        }
    }

    fn critical(entity: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            severity: HealthSeverity::Critical,
            entity: entity.into(),
            detail: detail.into(),
        }
    }
}

/// Health of all durable entities. Live media attachment is intentionally
/// absent because evaluating stored state must never mount or probe devices.
pub struct DatabaseHealth {
    pub healthy_keys: usize,
    pub healthy_splits: usize,
    pub active_certificates: usize,
    pub healthy_active_certificates: usize,
    pub issues: Vec<HealthIssue>,
}

impl Key {
    pub fn has_required_replicas(&self) -> bool {
        self.backups.len() >= MIN_KEY_REPLICAS
    }
}

impl Split {
    pub fn distinct_backup_count(&self) -> usize {
        self.backups
            .iter()
            .map(|backup| backup.share)
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    pub fn has_all_configured_shares(&self) -> bool {
        self.distinct_backup_count() >= self.num_splits as usize
    }

    pub fn has_recovery_quorum(&self) -> bool {
        self.distinct_backup_count() >= self.min_splits as usize
    }
}

impl Db {
    /// Assess recoverability, verification freshness, certificate validity,
    /// and referential integrity using only durable database state.
    pub fn health_at(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<DatabaseHealth, Box<dyn Error>> {
        let mut health = DatabaseHealth {
            healthy_keys: 0,
            healthy_splits: 0,
            active_certificates: self
                .certs
                .iter()
                .filter(|cert| cert.retired_at.is_none())
                .count(),
            healthy_active_certificates: 0,
            issues: Vec::new(),
        };

        for key in &self.keys {
            let entity = format!("key {}", key.name);
            let references_valid = key
                .backups
                .iter()
                .all(|media| self.lookup_media(media).is_some());

            if !key.has_required_replicas() {
                let detail = format!(
                    "has {} complete copies; policy requires {MIN_KEY_REPLICAS}",
                    key.backups.len()
                );
                health.issues.push(if key.backups.is_empty() {
                    HealthIssue::critical(entity.clone(), detail)
                } else {
                    HealthIssue::warning(entity.clone(), detail)
                });
            }

            let verification = key.verification_status_at(now);
            add_verification_issue(&mut health.issues, &entity, verification);
            if key.has_required_replicas()
                && references_valid
                && verification == PrivateEntityVerification::Complete
            {
                health.healthy_keys += 1;
            }

            for media in &key.backups {
                if self.lookup_media(media).is_none() {
                    health.issues.push(HealthIssue::critical(
                        entity.clone(),
                        format!("references unknown media {media}"),
                    ));
                }
            }
        }

        for split in &self.splits {
            let entity = format!("split {}", split.label);
            let references_valid = self.lookup_key(&split.key).is_some()
                && split
                    .backups
                    .iter()
                    .all(|backup| self.lookup_media(&backup.media).is_some());
            let available_shares = split.distinct_backup_count();

            if !split.has_recovery_quorum() {
                health.issues.push(HealthIssue::critical(
                    entity.clone(),
                    format!(
                        "only {available_shares} distinct shares are recorded; {} are required for recovery",
                        split.min_splits
                    ),
                ));
            } else if !split.has_all_configured_shares() {
                health.issues.push(HealthIssue::warning(
                    entity.clone(),
                    format!(
                        "only {available_shares} of {} configured shares are recorded",
                        split.num_splits
                    ),
                ));
            }

            let verification = split.verification_status_at(now);
            add_verification_issue(&mut health.issues, &entity, verification);
            if split.has_all_configured_shares()
                && references_valid
                && verification == PrivateEntityVerification::Complete
            {
                health.healthy_splits += 1;
            }

            if self.lookup_key(&split.key).is_none() {
                health.issues.push(HealthIssue::critical(
                    entity.clone(),
                    format!("references unknown key {}", split.key),
                ));
            }
            for backup in &split.backups {
                if self.lookup_media(&backup.media).is_none() {
                    health.issues.push(HealthIssue::critical(
                        entity.clone(),
                        format!(
                            "share {} references unknown media {}",
                            backup.share.0, backup.media
                        ),
                    ));
                }
            }
        }

        for cert in &self.certs {
            let entity = format!("certificate {}", cert.name);
            let key_exists = self.lookup_key(&cert.key).is_some();
            let issuer_exists = cert
                .issuer
                .as_ref()
                .is_none_or(|issuer| self.lookup_cert(issuer).is_some());

            if !key_exists {
                health.issues.push(HealthIssue::critical(
                    entity.clone(),
                    format!("references unknown key {}", cert.key),
                ));
            }
            if let Some(issuer) = &cert.issuer
                && !issuer_exists
            {
                health.issues.push(HealthIssue::critical(
                    entity.clone(),
                    format!("references unknown issuer {issuer}"),
                ));
            }

            match cert.validity_at(now) {
                Err(error) => health.issues.push(HealthIssue::critical(
                    entity,
                    format!("stored certificate PEM is invalid: {error}"),
                )),
                Ok(CertificateValidity::Retired) => {}
                Ok(CertificateValidity::NotYetValid) => health
                    .issues
                    .push(HealthIssue::critical(entity, "is not valid yet")),
                Ok(CertificateValidity::Expired) => health
                    .issues
                    .push(HealthIssue::critical(entity, "has expired")),
                Ok(CertificateValidity::ExpiringSoon) => {
                    health.issues.push(HealthIssue::warning(
                        entity,
                        format!("expires within {CERT_EXPIRY_WARNING_DAYS} days"),
                    ));
                }
                Ok(CertificateValidity::Current) => {
                    if key_exists && issuer_exists {
                        health.healthy_active_certificates += 1;
                    }
                }
            }
        }

        Ok(health)
    }
}

fn add_verification_issue(
    issues: &mut Vec<HealthIssue>,
    entity: &str,
    status: PrivateEntityVerification,
) {
    match status {
        PrivateEntityVerification::Complete => {}
        PrivateEntityVerification::Degraded { verified, expected } => {
            issues.push(HealthIssue::warning(
                entity,
                format!("only {verified} of {expected} copies or shares have fresh verification"),
            ));
        }
        PrivateEntityVerification::NotVerified => issues.push(HealthIssue::critical(
            entity,
            "has no usable complete verification within the freshness interval",
        )),
    }
}

fn record_verification(
    verifications: &mut Vec<KeyCopyVerification>,
    media: Name<Media>,
    verified_at: chrono::DateTime<chrono::Utc>,
) {
    if let Some(existing) = verifications
        .iter_mut()
        .find(|verification| verification.media == media)
    {
        existing.verified_at = verified_at;
    } else {
        verifications.push(KeyCopyVerification { media, verified_at });
    }
}

struct MetaEntry {
    key: String,
    value: String,
}

impl crate::ui::ListItem for MetaEntry {
    fn column_names() -> &'static [&'static str] {
        &["key", "value"]
    }

    fn get_field(&self, col: usize) -> String {
        match col {
            0 => self.key.clone(),
            1 => self.value.clone(),
            _ => "".into(),
        }
    }
}

#[derive(Clone)]
pub struct Meta {
    pub metadata: HashMap<String, String>,
}

impl Meta {
    pub fn new() -> Self {
        Meta {
            metadata: HashMap::<String, String>::new(),
        }
    }

    /// Present metadata in stable key order.
    pub fn properties(&self) -> crate::ui::PropertyList {
        let mut entries = self.metadata.iter().collect::<Vec<_>>();
        entries.sort_by(|(left, _), (right, _)| left.cmp(right));
        crate::ui::PropertyList::new(
            entries
                .into_iter()
                .map(|(key, value)| crate::ui::Property::new(key, value)),
        )
    }

    pub async fn manage<Ui: crate::ui::Ui>(&mut self, ui: &Ui, args: &MetaSetArgs) {
        match &args.command {
            MetaCommand::Remove { key } => {
                self.metadata.remove(key);
            }
            MetaCommand::Set { key, value } => {
                self.metadata.insert(key.clone(), value.clone());
            }
            MetaCommand::Show { key, list_options } => {
                let entries: Vec<MetaEntry> = if key.is_empty() {
                    self.metadata
                        .iter()
                        .map(|(key, value)| MetaEntry {
                            key: key.clone(),
                            value: value.clone(),
                        })
                        .collect()
                } else {
                    key.iter()
                        .map(|key| {
                            let value = match self.metadata.get(key) {
                                None => String::new(),
                                Some(v) => v.clone(),
                            };
                            MetaEntry {
                                key: key.clone(),
                                value,
                            }
                        })
                        .collect()
                };
                ui.list(entries).with_options(list_options).display().await
            }
        }
    }
}

impl Serialize for Meta {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.metadata.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Meta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let metadata = HashMap::<String, String>::deserialize(deserializer)?;
        Ok(Meta { metadata })
    }
}

#[derive(clap::Args)]
pub struct MetaSetArgs {
    #[command(subcommand)]
    command: MetaCommand,
}

#[derive(clap::Subcommand)]
pub enum MetaCommand {
    /// Remove a metadata value
    Remove {
        #[arg(long)]
        key: String,
    },

    /// Set a metadata value
    Set {
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: String,
    },

    /// Show metadata values
    Show {
        #[arg(long)]
        key: Vec<String>,
        #[command(flatten)]
        list_options: crate::util::ListOptions,
    },
}

// Traits
pub trait Entity: Any {
    fn kind(&self) -> &'static str;
    fn emoji(&self) -> &'static str;
    fn name(&self) -> &String;
}

/// Routine redundancy remaining after one medium stops counting toward
/// recoverability. This is separate from verification freshness: a remaining
/// copy may exist without having been checked recently.
pub struct RedundancyAfterRemoval {
    pub remaining: usize,
    pub required: usize,
}

impl RedundancyAfterRemoval {
    pub fn is_sufficient(&self) -> bool {
        self.remaining >= self.required
    }
}

pub trait PrivateEntity: Entity {
    /// Healthy routine redundancy. For a split this deliberately means every
    /// configured share, rather than merely its emergency recovery quorum.
    fn required_replicas(&self) -> usize;

    fn backup_count_excluding(&self, media: &Name<Media>) -> usize;

    /// Calculate the entity-specific redundancy impact of removing a medium.
    /// Keys count complete copies; splits count distinct numbered shares, so
    /// duplicate placements of one share cannot satisfy the share target.
    fn redundancy_after_removing(&self, media: &Name<Media>) -> RedundancyAfterRemoval {
        RedundancyAfterRemoval {
            remaining: self.backup_count_excluding(media),
            required: self.required_replicas(),
        }
    }

    fn is_backed_up_on(&self, media: &Name<Media>) -> bool;
    fn last_verified_on(&self, media: &Name<Media>) -> Option<chrono::DateTime<chrono::Utc>>;
    fn verification_status_at(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> PrivateEntityVerification;

    fn needs_verification_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.verification_status_at(now) != PrivateEntityVerification::Complete
    }

    /// Whether this entity has a recently verified copy on some medium other
    /// than the one being removed from inventory.
    fn has_fresh_verification_elsewhere(
        &self,
        excluded_media: &Name<Media>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> bool;
}

impl Entity for Key {
    fn kind(&self) -> &'static str {
        "private key"
    }

    fn emoji(&self) -> &'static str {
        "🔑"
    }

    fn name(&self) -> &String {
        (&self.name).into()
    }
}

impl Entity for Cert {
    fn kind(&self) -> &'static str {
        "certificate"
    }

    fn emoji(&self) -> &'static str {
        "📜"
    }

    fn name(&self) -> &String {
        (&self.name).into()
    }
}

impl PrivateEntity for Key {
    fn required_replicas(&self) -> usize {
        MIN_KEY_REPLICAS
    }

    fn backup_count_excluding(&self, media: &Name<Media>) -> usize {
        self.backups.iter().filter(|backup| *backup != media).count()
    }

    fn is_backed_up_on(&self, media: &Name<Media>) -> bool {
        self.backups.contains(media)
    }

    fn last_verified_on(&self, media: &Name<Media>) -> Option<chrono::DateTime<chrono::Utc>> {
        self.verifications
            .iter()
            .find(|verification| &verification.media == media)
            .map(|verification| verification.verified_at)
    }

    fn verification_status_at(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> PrivateEntityVerification {
        let oldest_current = now - chrono::Duration::days(VERIFICATION_MAX_AGE_DAYS);
        let verified = self
            .backups
            .iter()
            .filter(|media| {
                self.last_verified_on(media)
                    .is_some_and(|verified_at| verified_at >= oldest_current)
            })
            .count();
        match (verified, self.backups.len()) {
            (0, _) => PrivateEntityVerification::NotVerified,
            (verified, expected) if verified == expected => PrivateEntityVerification::Complete,
            (verified, expected) => PrivateEntityVerification::Degraded { verified, expected },
        }
    }

    fn has_fresh_verification_elsewhere(
        &self,
        excluded_media: &Name<Media>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        let oldest_current = now - chrono::Duration::days(VERIFICATION_MAX_AGE_DAYS);
        self.backups.iter().any(|media| {
            media != excluded_media
                && self
                    .last_verified_on(media)
                    .is_some_and(|verified_at| verified_at >= oldest_current)
        })
    }
}

impl Entity for Split {
    fn kind(&self) -> &'static str {
        "key split"
    }

    fn emoji(&self) -> &'static str {
        "🧩"
    }

    fn name(&self) -> &String {
        (&self.label).into()
    }
}

impl PrivateEntity for Split {
    fn required_replicas(&self) -> usize {
        self.num_splits as usize
    }

    fn backup_count_excluding(&self, media: &Name<Media>) -> usize {
        self.backups
            .iter()
            .filter(|backup| &backup.media != media)
            .map(|backup| backup.share)
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    fn is_backed_up_on(&self, media: &Name<Media>) -> bool {
        self.backups.iter().any(|backup| &backup.media == media)
    }

    fn last_verified_on(&self, media: &Name<Media>) -> Option<chrono::DateTime<chrono::Utc>> {
        self.verifications
            .iter()
            .filter(|verification| {
                verification
                    .shares
                    .iter()
                    .any(|share| &share.media == media)
            })
            .map(|verification| verification.verified_at)
            .max()
    }

    fn verification_status_at(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> PrivateEntityVerification {
        let oldest_current = now - chrono::Duration::days(VERIFICATION_MAX_AGE_DAYS);
        let verified = self
            .verifications
            .iter()
            .filter(|verification| verification.verified_at >= oldest_current)
            .flat_map(|verification| verification.shares.iter().map(|share| share.share))
            .collect::<std::collections::HashSet<_>>()
            .len();
        let expected = self.num_splits as usize;

        if verified == expected && expected != 0 {
            PrivateEntityVerification::Complete
        } else if verified >= self.min_splits as usize {
            PrivateEntityVerification::Degraded { verified, expected }
        } else {
            PrivateEntityVerification::NotVerified
        }
    }

    fn has_fresh_verification_elsewhere(
        &self,
        excluded_media: &Name<Media>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        let oldest_current = now - chrono::Duration::days(VERIFICATION_MAX_AGE_DAYS);
        let shares_being_removed = self
            .backups
            .iter()
            .filter(|backup| &backup.media == excluded_media)
            .map(|backup| backup.share)
            .collect::<std::collections::HashSet<_>>();

        shares_being_removed.iter().all(|share_number| {
            self.backups.iter().any(|backup| {
                backup.share == *share_number
                    && &backup.media != excluded_media
                    && self.verifications.iter().any(|verification| {
                        verification.verified_at >= oldest_current
                            && verification.shares.contains(backup)
                    })
            })
        })
    }
}

// Impls
impl Db {
    #[allow(dead_code)]
    fn entities(&self) -> impl Iterator<Item = &dyn Entity> {
        itertools::chain!(
            self.keys.iter().map(|x| x as &dyn Entity),
            self.certs.iter().map(|x| x as &dyn Entity),
            self.splits.iter().map(|x| x as &dyn Entity)
        )
    }

    fn private_entities(&self) -> impl Iterator<Item = &dyn PrivateEntity> {
        itertools::chain!(
            self.keys.iter().map(|x| x as &dyn PrivateEntity),
            self.splits.iter().map(|x| x as &dyn PrivateEntity)
        )
    }

    pub(crate) fn find_media_entities(
        &self,
        media: &Name<Media>,
    ) -> impl Iterator<Item = &dyn PrivateEntity> {
        self.private_entities()
            .filter(|entity| entity.is_backed_up_on(media))
    }
}
