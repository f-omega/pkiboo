use std::{error::Error, path::{Path, PathBuf}, sync::Arc};
use serde::{Serialize, Deserialize};
use procfs::process::Process;
use crate::cli_common;
use crate::util::Name;
use backend::{FileSystem, Media};

mod create;
mod repair;
mod list;
mod meta;
mod show;

mod physical;
mod manifest;
pub mod backend;
pub mod udisks;

pub use manifest::OpenManifest;


#[derive(PartialEq, Clone, Serialize, Deserialize)]
pub enum MediaId {
    PhysicalMedia {
        #[serde(flatten)]
        fingerprint: physical::PhysicalFingerprint,

        #[serde(skip)]
        path: Option<PathBuf>
    }
}

impl std::fmt::Display for MediaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaId::PhysicalMedia{ fingerprint: fp, .. } => fp.fmt(f)
        }
    }
}

impl MediaId {
    pub async fn open_backend(&self) -> Result<Arc<dyn Media>, Box<dyn Error>> {
        match self {
            MediaId::PhysicalMedia{ fingerprint: fp, path: mount_point } => {
                let mut fs = FileSystem::new(fp.clone());
                if let Some(p) = mount_point {
                    fs = fs.with_path(p.clone());
                }
                Ok(Arc::new(fs))
            }
        }
    }
}

/// Command spec for media on the cli
#[derive(clap::Args)]
pub struct MediaRef {
    #[arg(long)]
    media: String,
}

impl MediaRef {
    pub fn new(media: String) -> Self {
        Self { media }
    }

    /// Resolve a spec in the database
    pub fn resolve(&self, db: &crate::pkiboo::Db) -> Result<MediaId, Box<dyn Error>> {
        // By default we look up by name
        match db.lookup_media(&Name::new(self.media.clone())) {
            Some(media) => Ok(media.id.clone()),
            None => {
                let info = physical::get_device_info(&Path::new(&self.media))?;
                let media = db.media.iter().find(|n| {
                    if let MediaId::PhysicalMedia{ fingerprint: f, .. } = &n.id {
                        f == &info.fingerprint
                    } else {
                        false
                    }
                }).ok_or::<String>(format!("Could not find media {}", self.media).into())?;
                Ok(media.id.clone())
            }
        }
    }
}

#[derive(clap::Parser)]
pub(crate) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command  {
    /// Create a new media from a removable device
    Create(create::Args),

    /// List all registered media
    List(list::Args),

    /// Show detailed information about media
    Show(show::Args),

    /// Repair a disk that may have been wiped
    Repair(repair::Args),

    /// Manage metadata on the medium
    Meta(meta::Args),

    /// Rename the medium
    Rename(meta::Rename),
}

pub(crate) async fn main<Ui: crate::Ui>(boo: &crate::pkiboo::PkiBoo<Ui>, args: &Args) -> Result<(), Box<dyn Error>> {
    match &args.command {
        Command::Create(c) => create::main(boo, args, c).await,
        Command::List(c) => list::main(boo, args, c).await,
        Command::Meta(c) => meta::main(boo, args, c).await,
        Command::Repair(c) => repair::main(boo, args, c).await,
        Command::Rename(c) => meta::rename(boo, args, c).await,
        Command::Show(c) => show::main(boo, args, c).await
    }
}

// Items

pub(crate) trait MediaItem {
    /// Emoji icon for this item kind
    fn emoji(&self) -> &'static str;

    /// Human friendly description of this item
    fn human_friendly(&self) -> String;

    /// Where this ought to have been found, relative to the PKI directory
    fn path(&self) -> PathBuf;
}
