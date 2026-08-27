use crate::media::backend::{Media as MediaBackend, ReleaseResult};
use crate::pkiboo::{Key, Media};
use crate::ui::{ListItem, ListView, PaneStarterExt, Presenter, Task};
use crate::util::Name;
use futures::{StreamExt, stream::FuturesUnordered};
use openssl::pkey::{PKey, Private};
use secrecy::ExposeSecret;
use std::{error::Error, sync::Arc};

#[derive(clap::Args)]
pub struct Args {
    /// Key whose complete copies should be verified
    #[arg(long)]
    key: Name<Key>,

    /// Media to verify; repeat to select more than one (defaults to all complete copies)
    #[arg(long)]
    media: Vec<Name<Media>>,
}

struct VerificationResult {
    media: String,
    status: &'static str,
    detail: String,
}

impl ListItem for VerificationResult {
    fn column_names() -> &'static [&'static str] {
        &["media", "status", "detail"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.media.clone(),
            1 => self.status.into(),
            2 => self.detail.clone(),
            _ => String::new(),
        }
    }
}

async fn verify_copy(
    db: &crate::pkiboo::Db,
    key: &Key,
    backend: Arc<dyn MediaBackend>,
) -> Result<(), Box<dyn Error>> {
    backend.wait_for_available().await?;

    let manifest = crate::media::OpenManifest::new(backend).await?;
    let private_pem = manifest
        .read_verified(db, &key.key_path())
        .await?
        .ok_or_else(|| format!("Media does not contain a complete copy of key {}", key.name))?;
    let private_key: PKey<Private> = PKey::private_key_from_pem(private_pem.expose_secret())?;
    let expected_public_key = key.load_public_key()?;

    if !private_key.public_eq(&expected_public_key) {
        return Err(format!("Private key copy does not match public key {}", key.name).into());
    }

    Ok(())
}

async fn release(backend: &Arc<dyn MediaBackend>) -> Result<String, Box<dyn Error>> {
    Ok(match backend.release().await? {
        ReleaseResult::Released => "verified and safe to remove".into(),
        ReleaseResult::ExternalMount(path) => {
            format!("verified; unmount {} before removal", path.display())
        }
        ReleaseResult::NotMounted => "verified".into(),
    })
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _key: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let key = db
        .lookup_key(&args.key)
        .ok_or_else(|| format!("Key {} not found", args.key))?
        .clone();

    let requested_media = if args.media.is_empty() {
        key.backups.clone()
    } else {
        args.media.clone()
    };
    if requested_media.is_empty() {
        return Err(format!("Key {} has no complete copies to verify", key.name).into());
    }

    let media = requested_media
        .iter()
        .map(|name| {
            db.lookup_media(name)
                .cloned()
                .ok_or_else(|| format!("Media {name} not found"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut results = (0..media.len()).map(|_| None).collect::<Vec<_>>();
    let mut interrupted = false;
    let cancel = tokio_util::sync::CancellationToken::new();
    let workers = media
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, medium)| {
            let cancel = cancel.child_token();
            let key = key.clone();
            let db = db.clone();
            async move {
                let task = boo
                    .ui()
                    .start_task(format!("Verify {} on {}", key.name, medium.label))
                    .await;
                task.set_message(format!("Insert media {}", medium.label))
                    .await;

                let backend = match medium.id.open_backend().await {
                    Ok(backend) => backend,
                    Err(error) => {
                        task.mark_error(error.to_string()).await;
                        return (
                            index,
                            VerificationResult {
                                media: medium.label.to_string(),
                                status: "failed",
                                detail: error.to_string(),
                            },
                        );
                    }
                };

                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = backend.release().await;
                        task.mark_cancelled("Interrupted".into()).await;
                        (index, VerificationResult {
                            media: medium.label.to_string(),
                            status: "not verified",
                            detail: "interrupted".into(),
                        })
                    }
                    verification = verify_copy(&db, &key, backend.clone()) => {
                        match verification {
                            Ok(()) => match release(&backend).await {
                                Ok(detail) => {
                                    task.set_message(detail.clone()).await;
                                    task.mark_complete().await;
                                    (index, VerificationResult {
                                        media: medium.label.to_string(),
                                        status: "verified",
                                        detail,
                                    })
                                }
                                Err(error) => {
                                    task.mark_error(error.to_string()).await;
                                    (index, VerificationResult {
                                        media: medium.label.to_string(),
                                        status: "failed",
                                        detail: format!("Verified copy, but could not release media: {error}"),
                                    })
                                }
                            }
                            Err(error) => {
                                let _ = backend.release().await;
                                task.mark_error(error.to_string()).await;
                                (index, VerificationResult {
                                    media: medium.label.to_string(),
                                    status: "failed",
                                    detail: error.to_string(),
                                })
                            }
                        }
                    }
                }
            }
        })
        .collect::<FuturesUnordered<_>>();
    tokio::pin!(workers);

    while !workers.is_empty() {
        tokio::select! {
            completed = workers.next() => {
                if let Some((index, result)) = completed {
                    results[index] = Some(result);
                }
            }
            signal = tokio::signal::ctrl_c(), if !interrupted => {
                signal?;
                interrupted = true;
                cancel.cancel();
            }
        }
    }

    let results = results
        .into_iter()
        .map(|result| result.expect("every verification worker returns a result"))
        .collect::<Vec<_>>();
    let failed = results.iter().any(|result| result.status == "failed");

    boo.ui()
        .pane(
            "Verification report".into(),
            async |pane| -> Result<(), Box<dyn Error>> {
                pane.list(results).display().await;
                Ok(())
            },
        )
        .await?;

    if interrupted {
        Err("Verification interrupted".into())
    } else if failed {
        Err("One or more key copies could not be verified".into())
    } else {
        Ok(())
    }
}
