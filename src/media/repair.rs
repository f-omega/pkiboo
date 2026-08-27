use super::assessment::MediaAssessment;
use super::manifest::{OpenManifest, OpenManifestError};
use crate::ui::{Task, TaskStarterExt, UiKeypairExt};
use futures::{StreamExt, future::join_all, stream::FuturesUnordered};
use std::collections::HashSet;
use std::error::Error;

#[derive(clap::Parser)]
pub struct Args {
    /// Medium whose expected contents should be repaired
    #[command(flatten)]
    media: super::MediaRef,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::pkiboo::PkiBoo<Ui>,
    _media: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let media_id = args.media.resolve(&db)?;
    let media = db
        .lookup_media_by_id(&media_id)
        .ok_or_else(|| format!("Could not find media {media_id}"))?
        .clone();

    let assessment = boo
        .ui()
        .task(format!("Repair media {}", media.label), async |task| {
            let backend = media.id.open_backend().await?;

            // Keep the destination available while individual source copies
            // are loaded. Mount operations remain serialized by the backend,
            // but key-source waiters can still run concurrently.
            let repair = async {
                backend.wait_for_available().await?;

                let before = MediaAssessment::collect(&db, &media, backend.clone()).await?;
                let verified_paths = before
                    .verified_files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<HashSet<_>>();
                let keys_to_repair = db
                    .keys
                    .iter()
                    .filter(|key| {
                        key.backups.contains(&media.label)
                            && !verified_paths.contains(&key.key_path())
                    })
                    .cloned()
                    .collect::<Vec<_>>();

                // A missing or syntactically invalid manifest can be rebuilt
                // from the database and other verified key copies. A backend
                // read failure is operational, so do not overwrite anything.
                let mut manifest = match OpenManifest::new(backend.clone()).await {
                    Ok(manifest) => manifest,
                    Err(OpenManifestError::Missing | OpenManifestError::Invalid(_)) => {
                        OpenManifest::create(backend.clone())
                    }
                    Err(OpenManifestError::Read(error)) => return Err(error),
                };

                // Start every key load together. Each load independently waits
                // for all known source media, so inserting one medium can
                // satisfy several pending keys before the operator removes it.
                let db_ref = &db;
                let mut loads = FuturesUnordered::new();
                for key in keys_to_repair {
                    let loader = task.clone();
                    loads.push(async move {
                        let result = loader
                            .load_private_key_with_source(db_ref, &key.name)
                            .await;
                        (key, result)
                    });
                }

                let mut loaded_sources = Vec::new();
                let mut load_errors = Vec::new();
                let mut interrupted = false;
                while !loads.is_empty() {
                    let next = tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            interrupted = true;
                            None
                        }
                        result = loads.next() => result,
                    };

                    let Some((key, result)) = next else {
                        break;
                    };
                    match result {
                        Ok(source) => loaded_sources.push((key, source)),
                        Err(error) => load_errors.push(error.to_string()),
                    }
                }
                drop(loads);

                if interrupted {
                    task.set_message(
                        "Interrupted; finishing repairs for keys already loaded".into(),
                    )
                    .await;
                } else if !load_errors.is_empty() {
                    task.set_message(format!(
                        "{} keys could not be loaded; continuing with partial repair",
                        load_errors.len()
                    ))
                    .await;
                }

                let mut loaded_keys = Vec::new();
                for (key, source) in loaded_sources {
                    let loaded = crate::keypair::LoadedKey::new(
                        crate::keypair::PrivateKey {
                            algo: key.algorithm.clone(),
                            pkey: source.private_key,
                        },
                        key.clone(),
                    );
                    loaded_keys.push((key, loaded, source.backend));
                }

                // All private keys are now in memory, so source media can be
                // released before the destination manifest is modified.
                let release_results = join_all(
                    loaded_keys
                        .iter()
                        .map(|(_, _, backend)| backend.release()),
                )
                .await;
                if let Some(error) = release_results.into_iter().find_map(Result::err) {
                    return Err(error);
                }

                for (key, loaded, _) in loaded_keys {
                    if let Err(error) = loaded.replace_on_media(&mut manifest).await {
                        load_errors.push(format!("Could not restore {}: {error}", key.name));
                        continue;
                    }
                    // Save after each restored key. If a later source cannot
                    // be supplied, repairs already completed remain durable.
                    if let Err(error) = manifest.save().await {
                        load_errors.push(format!(
                            "Could not save manifest after restoring {}: {error}",
                            key.name
                        ));
                        continue;
                    }
                }

                // This also materializes an empty manifest when a damaged
                // medium currently has no expected private-key contents.
                manifest.save().await?;

                let after = MediaAssessment::collect(&db, &media, backend.clone()).await?;

                // Repair is authoritative: anything that verifies remains a
                // known copy, while anything still absent or invalid no longer
                // counts as a backup on this medium. Unlike verify, repair has
                // no --no-store mode.
                let verified_paths = after
                    .verified_files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<HashSet<_>>();
                let updates = db
                    .keys
                    .iter()
                    .filter(|key| key.backups.contains(&media.label))
                    .cloned()
                    .collect::<Vec<_>>();
                let mut transaction = db.transaction();
                for mut key in updates {
                    if verified_paths.contains(&key.key_path()) {
                        key.record_verification(media.label.clone(), after.checked_at);
                    } else {
                        key.remove_backup(&media.label);
                    }
                    transaction.update_key(key)?;
                }
                drop(transaction);

                // Write the reconciled database, not the pre-repair view.
                db.backup(backend.clone()).await?;
                Ok::<_, Box<dyn Error>>(after)
            }
            .await;

            // Only mounts acquired by Pkiboo are unmounted. An existing user
            // mount remains under the user's control.
            let release = backend.release().await;
            match (repair, release) {
                (Ok(result), Ok(_)) => Ok(result),
                (Err(error), _) => Err(error),
                (Ok(_), Err(error)) => Err(error),
            }
        })
        .await?;

    super::verify::display_assessment(boo, assessment).await
}
