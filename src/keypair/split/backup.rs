use crate::{
    media::OpenManifest,
    pkiboo::{Media, ShareNumber, Split, SplitBackup},
    ui::{Task, TaskStarterExt},
    util::Name,
};
use futures::{StreamExt, stream::FuturesUnordered};
use secrecy::ExposeSecret;
use std::{collections::HashSet, error::Error, sync::Arc};

#[derive(clap::Args)]
pub struct Args {
    /// Recovery split containing the share
    #[arg(long)]
    split: Name<Split>,

    /// Numbered share to copy
    #[arg(long)]
    share: u32,

    /// Destination media; repeat to create more than one replica
    #[arg(long, required = true)]
    media: Vec<Name<Media>>,

    /// Allow multiple shares or a complete copy of the key on one medium
    #[arg(long)]
    force: bool,
}

async fn release(backend: &Arc<dyn crate::media::backend::Media>) -> Result<(), Box<dyn Error>> {
    backend.release().await?;
    Ok(())
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _split_args: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let split = db
        .lookup_split(&args.split)
        .ok_or_else(|| format!("Split {} not found", args.split))?
        .clone();
    let share = ShareNumber(args.share);

    let mut source_names = HashSet::new();
    let source_media = split
        .backups
        .iter()
        .filter(|backup| backup.share == share)
        .filter(|backup| source_names.insert(backup.media.to_string()))
        .map(|backup| {
            db.lookup_media(&backup.media)
                .cloned()
                .ok_or_else(|| format!("Source media {} not found", backup.media))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if source_media.is_empty() {
        return Err(format!("Share {} has no recorded source placement", args.share).into());
    }

    let mut selected = HashSet::new();
    let mut destinations = Vec::with_capacity(args.media.len());
    for name in &args.media {
        if !selected.insert(name.to_string()) {
            return Err(format!("Destination media {name} was selected more than once").into());
        }
        if source_media.iter().any(|source| &source.label == name) {
            return Err(format!(
                "Media {name} already contains this share and cannot be a destination"
            )
            .into());
        }
        let destination = db
            .lookup_media(name)
            .ok_or_else(|| format!("Destination media {name} not found"))?
            .clone();

        let existing = db
            .splits_for_key(&split.key)
            .flat_map(|candidate| {
                candidate
                    .backups
                    .iter()
                    .filter(|backup| backup.media == destination.label)
                    .map(move |backup| (candidate.label.clone(), backup.share))
            })
            .collect::<Vec<_>>();
        if existing.iter().any(|(existing_split, existing_share)| {
            existing_split == &split.label && existing_share == &share
        }) {
            return Err(format!(
                "Media {} already contains share {} of split {}",
                destination.label, share.0, split.label
            )
            .into());
        }

        let has_complete_key = db
            .lookup_key(&split.key)
            .is_some_and(|key| key.backups.contains(&destination.label));
        if has_complete_key && !args.force {
            return Err(format!(
                "Media {} already contains a complete copy of key {}; pass --force to colocate it with a share",
                destination.label, split.key
            )
            .into());
        }
        if !existing.is_empty() && !args.force {
            return Err(format!(
                "Media {} already contains another share of key {}; pass --force to colocate multiple shares",
                destination.label, split.key
            )
            .into());
        }
        if has_complete_key || !existing.is_empty() {
            crate::cli_common::warn(format!(
                "Forcing share {} onto {} despite existing secret material for key {}",
                share.0, destination.label, split.key
            ));
        }
        destinations.push(destination);
    }

    let share_path = split.share(share).path();
    let (_source_media, source_backend, source_manifest) =
        boo.ui()
            .task("Wait for a source share".into(), async |task| {
                task.set_message(format!(
                    "Insert any media containing share {}: {}",
                    share.0,
                    source_media
                        .iter()
                        .map(|media| media.label.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .await;

                let mut waits = source_media
                    .into_iter()
                    .map(|media| {
                        let db = &db;
                        let share_path = &share_path;
                        async move {
                            let backend = media.id.open_backend().await?;
                            backend.wait_for_available().await?;
                            let manifest = OpenManifest::new(backend.clone()).await?;
                            let contents = manifest
                                .read_verified(db, share_path)
                                .await?
                                .ok_or_else(|| {
                                    format!(
                                        "Media {} does not contain {}",
                                        media.label,
                                        share_path.display()
                                    )
                                })?;
                            let decoded: super::share::ShamirShareFile =
                                yaml_serde::from_slice(contents.expose_secret())?;
                            if u32::from(decoded.x) != share.0 {
                                return Err(format!(
                                    "Media {} contains share {}, not requested share {}",
                                    media.label, decoded.x, share.0
                                )
                                .into());
                            }
                            Ok::<_, Box<dyn Error>>((media, backend, manifest))
                        }
                    })
                    .collect::<FuturesUnordered<_>>();
                let mut errors = Vec::new();
                while let Some(result) = waits.next().await {
                    match result {
                        Ok(source) => return Ok(source),
                        Err(error) => errors.push(error.to_string()),
                    }
                }
                Err(format!(
                    "No recorded source media provided a valid share{}",
                    if errors.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", errors.join("; "))
                    }
                )
                .into())
            })
            .await?;

    let copies_result = async {
        for destination in destinations {
            let backend = destination.id.open_backend().await?;
            let write = boo
                .ui()
                .task(
                    format!("Copy share to {}", destination.label),
                    async |task| {
                        task.set_message(format!("Insert media {}", destination.label))
                            .await;
                        backend.wait_for_available().await?;
                        let mut manifest = OpenManifest::new(backend.clone()).await?;
                        manifest
                            .copy_verified_file_from(&db, &source_manifest, &share_path, false)
                            .await?;
                        manifest.save().await?;
                        Ok::<_, Box<dyn Error>>(())
                    },
                )
                .await;

            if write.is_ok() {
                let mut tx = db.transaction();
                let target = tx
                    .splits
                    .iter_mut()
                    .find(|candidate| candidate.label == split.label)
                    .expect("validated split disappeared");
                target.backups.push(SplitBackup {
                    share,
                    media: destination.label.clone(),
                });
            }

            let hint_error = if write.is_ok() {
                db.write_recovery_hint(backend.clone()).await.err()
            } else {
                None
            };
            let release_result = release(&backend).await;
            write?;
            if let Some(error) = hint_error {
                crate::cli_common::warn(format!(
                    "Share backup succeeded, but its recovery hint was not refreshed: {error}"
                ));
            }
            release_result?;
        }
        Ok::<_, Box<dyn Error>>(())
    }
    .await;

    let source_release = release(&source_backend).await;
    copies_result?;
    source_release?;
    Ok(())
}
