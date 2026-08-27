use crate::pkiboo::Key;
use crate::ui::{ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView};
use crate::util::Name;
use futures::future::try_join_all;
use openssl::hash::{MessageDigest, hash};
use std::error::Error;
use std::io::Write;

struct CompleteCopy {
    media: String,
    trusted: bool,
    last_verified: String,
}

impl crate::ui::ListItem for CompleteCopy {
    fn column_names() -> &'static [&'static str] {
        &["media", "trusted", "last verified"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.media.clone(),
            1 => self.trusted.to_string(),
            2 => self.last_verified.clone(),
            _ => String::new(),
        }
    }
}

#[derive(clap::Args)]
pub struct Args {
    /// Name of the key
    #[arg(long)]
    key: Name<Key>,

    /// Print only the PEM-encoded public key
    #[arg(long)]
    pem: bool,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _key: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let key = db
        .lookup_key(&args.key)
        .ok_or_else(|| format!("Key {} not found", args.key))?;

    if args.pem {
        std::io::stdout()
            .lock()
            .write_all(key.public_key.as_bytes())?;
        return Ok(());
    }

    let public_key = key.load_public_key()?;
    let fingerprint = hash(MessageDigest::sha256(), &public_key.public_key_to_der()?)?
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":");

    let details = boo.ui().pane(
        "Key details".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.property_list(PropertyList::new([
                Property::new("Name", key.name.to_string()),
                Property::new("Algorithm", key.algorithm.to_string()),
                Property::new("Fingerprint", fingerprint),
            ]))
            .display()
            .await;
            Ok(())
        },
    );

    let backup_media = db
        .media
        .iter()
        .filter(|media| key.backups.contains(&media.label))
        .map(|media| CompleteCopy {
            media: media.label.to_string(),
            trusted: media.trusted,
            last_verified: key
                .verifications
                .iter()
                .find(|verification| verification.media == media.label)
                .map(|verification| verification.verified_at.to_rfc3339())
                .unwrap_or_else(|| "never".into()),
        })
        .collect::<Vec<_>>();
    let copies = boo.ui().pane(
        "Complete copies".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.list(backup_media).display().await;
            Ok(())
        },
    );

    let metadata = boo.ui().pane(
        "Metadata".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.property_list(key.meta.properties()).display().await;
            Ok(())
        },
    );

    try_join_all([details, copies, metadata]).await?;
    Ok(())
}
