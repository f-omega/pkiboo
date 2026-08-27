use crate::pkiboo::Key;
use crate::ui::{ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView};
use crate::util::Name;
use futures::future::try_join;
use openssl::hash::{MessageDigest, hash};
use std::error::Error;
use std::io::Write;

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
        .cloned()
        .collect::<Vec<_>>();
    let copies = boo.ui().pane(
        "Complete copies".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.list(backup_media).display().await;
            Ok(())
        },
    );

    try_join(details, copies).await?;
    Ok(())
}
