use crate::cli_common::CliBackend;
use crate::ui::{ListView, Ui};
use crate::util::Name;
use itertools::Itertools;
use resolve_path::PathResolveExt;
use serde::{Deserialize, Serialize};
use std::any::Any;
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

    pub async fn backup(
        &self,
        media: Arc<dyn crate::media::backend::Media>,
    ) -> Result<(), Box<dyn Error>> {
        let s = yaml_serde::to_string(self)?;
        media.put(&DB_KEY.into(), &s.into_bytes()).await?;
        Ok(())
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
    algorithm: crate::keypair::Algorithm,

    /// PEM-encoded public key
    pub public_key: String,

    pub meta: Meta,

    pub backups: Vec<Name<Media>>,
}

impl Key {
    pub fn new(name: Name<Self>, algorithm: crate::keypair::Algorithm, public_key: String) -> Self {
        Key {
            name,
            algorithm,
            meta: Meta::new(),
            backups: Vec::new(),
            public_key,
        }
    }

    pub fn add_backup(&mut self, media: Name<Media>) {
        if !self.backups.contains(&media) {
            self.backups.push(media)
        }
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

    pub meta: Meta,
}

impl crate::ui::ListItem for Cert {
    fn column_names() -> &'static [&'static str] {
        &["name", "key", "issuer", "created"]
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

    backups: Vec<Name<Media>>,
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

pub trait PrivateEntity: Entity {
    /// private entities can be backed up to media and should be able to tell us where they are
    fn backups(&self) -> &[Name<Media>];
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
    fn backups(&self) -> &[Name<Media>] {
        &self.backups
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
    fn backups(&self) -> &[Name<Media>] {
        &self.backups
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
            .filter(|e| e.backups().contains(media))
    }
}
