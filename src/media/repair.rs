use super::assessment::MediaAssessment;
use super::manifest::{OpenManifest, OpenManifestError};
use crate::ui::{TaskStarterExt, UiKeypairExt};
use futures::future::join_all;
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

    let (assessment, repaired_keys) = boo
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
                let loads = join_all(keys_to_repair.into_iter().map(|key| {
                    let loader = task.clone();
                    async move {
                        let result = loader
                            .load_private_key_with_source(db_ref, &key.name)
                            .await;
                        (key, result)
                    }
                }))
                .await;

                let mut loaded_sources = Vec::new();
                let mut load_errors = Vec::new();
                for (key, result) in loads {
                    match result {
                        Ok(source) => loaded_sources.push((key, source)),
                        Err(error) => load_errors.push(error.to_string()),
                    }
                }

                if !load_errors.is_empty() {
                    // Other loads may have succeeded before one failed. Their
                    // media must still be released before returning the error.
                    let _ = join_all(
                        loaded_sources
                            .iter()
                            .map(|(_, source)| source.backend.release()),
                    )
                    .await;
                    return Err(load_errors.join("; ").into());
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

                let mut repaired = Vec::new();
                for (key, loaded, _) in loaded_keys {
                    loaded.replace_on_media(&mut manifest).await?;
                    // Save after each restored key. If a later source cannot
                    // be supplied, repairs already completed remain durable.
                    manifest.save().await?;
                    repaired.push(key.name);
                }

                // This also materializes an empty manifest when a damaged
                // medium currently has no expected private-key contents.
                manifest.save().await?;

                // Public database state is safe to recreate unconditionally.
                db.backup(backend.clone()).await?;
                let after = MediaAssessment::collect(&db, &media, backend.clone()).await?;
                Ok::<_, Box<dyn Error>>((after, repaired))
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

    // A restored copy has just passed the same complete assessment used by
    // media verify, so record fresh evidence only for repaired keys that are
    // present in the final verified-file set.
    let verified_paths = assessment
        .verified_files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    let verified_at = assessment.checked_at;
    for key_name in repaired_keys {
        let mut key = db
            .lookup_key(&key_name)
            .ok_or_else(|| format!("Could not find repaired key {key_name}"))?
            .clone();
        if verified_paths.contains(&key.key_path()) {
            key.record_verification(media.label.clone(), verified_at);
            db.transaction().update_key(key)?;
        }
    }

    super::verify::display_assessment(boo, assessment).await
}
