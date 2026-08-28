use crate::media::OpenManifest;
use crate::media::backend::ReleaseResult;
use crate::pkiboo::{Key, Media};
use crate::ui::{Task, TaskStarterExt, UiKeypairExt};
use crate::util::Name;
use std::error::Error;
use std::sync::Arc;

#[derive(clap::Parser)]
pub struct Args {
    /// Key to copy
    #[arg(long)]
    key: Name<Key>,

    /// Destination media for the new complete copy
    #[arg(long)]
    media: Name<Media>,

    /// Allow false redundancy with a recovery share on the same medium
    #[arg(long)]
    force: bool,

}

async fn release_media<T: Task>(
    task: &T,
    backend: &Arc<dyn crate::media::backend::Media>,
    media: &Name<Media>,
    wait_for_removal: bool,
) -> Result<(), Box<dyn Error>> {
    match backend.release().await? {
        ReleaseResult::Released => {
            task.set_message(format!("Media {media} can be safely removed"))
                .await;
        }
        ReleaseResult::ExternalMount(path) => {
            task.set_message(format!(
                "Unmount {} at {} and remove it",
                media,
                path.display()
            ))
            .await;
        }
        ReleaseResult::NotMounted => {
            task.set_message(format!("Media {media} can be removed"))
                .await;
        }
    }

    if wait_for_removal {
        backend.wait_for_removal().await?;
        task.set_message(format!("Media {media} was removed")).await;
    }

    Ok(())
}

pub(crate) async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _keypair: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let key = db
        .lookup_key(&args.key)
        .ok_or_else(|| format!("Key {} not found", args.key))?
        .clone();
    let destination = db
        .lookup_media(&args.media)
        .ok_or_else(|| format!("Media {} not found", args.media))?
        .clone();

    if key.backups.contains(&destination.label) {
        return Err(format!(
            "Key {} already has a complete copy on {}",
            key.name, destination.label
        )
        .into());
    }
    if !destination.trusted {
        return Err(format!(
            "Media {} is not trusted for complete key copies",
            destination.label
        )
        .into());
    }

    let shares_on_destination = db
        .splits_for_key(&key.name)
        .flat_map(|split| {
            split
                .backups
                .iter()
                .filter(|backup| backup.media == destination.label)
                .map(move |backup| format!("{} share {}", split.label, backup.share.0))
        })
        .collect::<Vec<_>>();
    if !shares_on_destination.is_empty() {
        let warning = format!(
            "Media {} already contains recovery shares of key {} ({}); a complete copy there is false redundancy",
            destination.label,
            key.name,
            shares_on_destination.join(", ")
        );
        if !args.force {
            return Err(format!("{warning}; pass --force to continue").into());
        }
        crate::cli_common::warn(warning);
    }

    let source = boo
        .ui()
        .task("Load source key".into(), async |task| {
            task.load_private_key_with_source(&db, &args.key).await
        })
        .await?;

    boo.ui()
        .task("Release source media".into(), async |task| {
            release_media(&task, &source.backend, &source.media, true).await
        })
        .await?;

    let loaded_key = super::LoadedKey::new(
        super::PrivateKey {
            algo: key.algorithm.clone(),
            pkey: source.private_key,
        },
        key.clone(),
    );
    let destination_backend = destination.id.open_backend().await?;

    let write_result = boo
        .ui()
        .task(
            format!("Write backup to {}", destination.label),
            async |task| {
                task.set_message(format!("Insert media {}", destination.label))
                    .await;
                destination_backend.wait_for_available().await?;
                destination_backend.setup().await?;

                let mut manifest = OpenManifest::new(destination_backend.clone()).await?;
                loaded_key.save_to_media(&mut manifest).await?;
                manifest.save().await?;

                task.set_message(format!("Key written to {}", destination.label))
                    .await;
                Ok(())
            },
        )
        .await;

    let update_result = if write_result.is_ok() {
        let mut updated_key = key;
        updated_key.add_backup(destination.label.clone());
        updated_key.record_verification(destination.label.clone(), chrono::Utc::now());
        db.transaction().update_key(updated_key).map(|_| ())
    } else {
        Ok(())
    };

    let hint_error = if write_result.is_ok() && update_result.is_ok() {
        db.write_recovery_hint(destination_backend.clone())
            .await
            .err()
    } else {
        None
    };

    // Release media even when writing, local-state update, or recovery-hint
    // synchronization failed after Pkiboo mounted it.
    let release_result = boo
        .ui()
        .task("Release destination media".into(), async |task| {
            release_media(&task, &destination_backend, &destination.label, false).await
        })
        .await;

    write_result?;
    update_result?;
    if let Some(error) = hint_error {
        crate::cli_common::warn(format!(
            "Backup succeeded, but the database recovery hint could not be written: {error}"
        ));
    }
    release_result?;

    Ok(())
}
