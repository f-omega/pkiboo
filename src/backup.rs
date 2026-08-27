use crate::media::backend::Media as MediaBackend;
use crate::pkiboo::Media;
use crate::ui::{ListItem, ListView, PaneStarterExt, Presenter, Task};
use crate::util::Name;
use futures::{StreamExt, stream::FuturesUnordered};
use std::error::Error;
use std::sync::Arc;

struct BackupResult {
    media: Name<Media>,
    status: &'static str,
    detail: String,
}

impl ListItem for BackupResult {
    fn column_names() -> &'static [&'static str] {
        &["media", "status", "detail"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.media.to_string(),
            1 => self.status.into(),
            2 => self.detail.clone(),
            _ => String::new(),
        }
    }
}

#[derive(clap::Args)]
pub struct Args {
    /// Destination media; repeat to choose more than one (defaults to all media)
    #[arg(long)]
    media: Vec<Name<Media>>,

    /// Stop after the database has been copied to the first available medium
    #[arg(long)]
    single: bool,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let selected = if args.media.is_empty() {
        db.media.clone()
    } else {
        args.media
            .iter()
            .map(|name| {
                db.lookup_media(name)
                    .cloned()
                    .ok_or_else(|| format!("Media {name} not found"))
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    if selected.is_empty() {
        return Err("No media are registered for database backup".into());
    }

    let cancel = tokio_util::sync::CancellationToken::new();
    let db_ref = &db;
    let mut workers = selected
        .into_iter()
        .map(|media| {
            let worker_cancel = cancel.child_token();
            async move {
                let task = boo
                    .ui()
                    .start_task(format!("Back up database to {}", media.label))
                    .await;
                task.set_message(format!("Insert media {}", media.label))
                    .await;

                let backend = match media.id.open_backend().await {
                    Ok(backend) => backend,
                    Err(error) => {
                        task.mark_error(error.to_string()).await;
                        return BackupResult {
                            media: media.label,
                            status: "failed",
                            detail: error.to_string(),
                        };
                    }
                };

                back_up_one(db_ref, &media.label, backend, worker_cancel, &task).await
            }
        })
        .collect::<FuturesUnordered<_>>();

    let mut results = Vec::new();
    let mut interrupted = false;

    while !workers.is_empty() {
        tokio::select! {
            result = workers.next() => {
                if let Some(result) = result {
                    let successful = result.status == "backed up";
                    results.push(result);
                    if args.single && successful {
                        cancel.cancel();
                    }
                }
            }
            signal = tokio::signal::ctrl_c(), if !interrupted => {
                signal?;
                interrupted = true;
                cancel.cancel();
            }
        }
    }

    let any_succeeded = results.iter().any(|result| result.status == "backed up");

    boo.ui()
        .pane(
            "Database backup report".into(),
            async |pane| -> Result<(), Box<dyn Error>> {
                pane.list(results).display().await;
                Ok(())
            },
        )
        .await?;

    if any_succeeded {
        Ok(())
    } else if interrupted {
        Err("Database backup interrupted".into())
    } else {
        Err("Database could not be backed up to any selected medium".into())
    }
}

async fn back_up_one<T: Task>(
    db: &crate::pkiboo::Db,
    media: &Name<Media>,
    backend: Arc<dyn MediaBackend>,
    cancel: tokio_util::sync::CancellationToken,
    task: &T,
) -> BackupResult {
    let backup = async {
        backend.wait_for_available().await?;
        db.write_recovery_hint(backend.clone()).await
    };

    let result = tokio::select! {
        _ = cancel.cancelled() => None,
        result = backup => Some(result),
    };
    let release = backend.release().await;

    match (result, release) {
        (None, _) => {
            task.mark_cancelled("Another destination completed or backup was interrupted".into())
                .await;
            BackupResult {
                media: media.clone(),
                status: "not backed up",
                detail: "cancelled".into(),
            }
        }
        (Some(Ok(())), Ok(_)) => {
            task.set_message(format!("Database backed up to {media}"))
                .await;
            task.mark_complete().await;
            BackupResult {
                media: media.clone(),
                status: "backed up",
                detail: "database recovery hint written".into(),
            }
        }
        (Some(Err(error)), _) | (Some(Ok(())), Err(error)) => {
            task.mark_error(error.to_string()).await;
            BackupResult {
                media: media.clone(),
                status: "failed",
                detail: error.to_string(),
            }
        }
    }
}
