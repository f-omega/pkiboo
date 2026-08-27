use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
use openssl::pkey::{PKey, Private};
use secrecy::ExposeSecret;
use std::error::Error;
use std::sync::Arc;

use crate::{
    media::OpenManifest,
    pkiboo::{Key, OpenedDb},
    util::Name,
};

use super::{Task, TaskStarterExt};

pub struct LoadedPrivateKey {
    pub private_key: PKey<Private>,
    pub media: Name<crate::pkiboo::Media>,
    pub backend: Arc<dyn crate::media::backend::Media>,
}

#[allow(dead_code)]
#[async_trait(?Send)]
pub trait UiKeypairExt: Task {
    /// Load a private key from the first complete copy that becomes available.
    ///
    /// One child task waits on each known backup. Once a valid copy is read,
    /// the remaining child tasks are cancelled and allowed to finish before
    /// this method returns.
    async fn load_private_key(
        &self,
        db: &OpenedDb,
        key_id: &Name<Key>,
    ) -> Result<PKey<Private>, Box<dyn Error>> {
        Ok(self
            .load_private_key_with_source(db, key_id)
            .await?
            .private_key)
    }

    /// Load a private key and retain the winning source medium so a workflow
    /// can release it before asking for another medium.
    async fn load_private_key_with_source(
        &self,
        db: &OpenedDb,
        key_id: &Name<Key>,
    ) -> Result<LoadedPrivateKey, Box<dyn Error>> {
        self.load_private_key_with_source_until(
            db,
            key_id,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
    }

    /// Load a private key while allowing the parent workflow to cancel all
    /// pending media waits cleanly.
    async fn load_private_key_with_source_until(
        &self,
        db: &OpenedDb,
        key_id: &Name<Key>,
        parent_cancel: tokio_util::sync::CancellationToken,
    ) -> Result<LoadedPrivateKey, Box<dyn Error>> {
        let key = db
            .lookup_key(key_id)
            .ok_or::<String>("Could not find key".into())?;
        if key.backups.is_empty() {
            return Err(format!("Key {key_id} has no complete copies").into());
        }

        let cancel = tokio_util::sync::CancellationToken::new();
        let private_key_path = key.key_path();
        let mut waiters = FuturesUnordered::new();

        for media_name in key.backups.iter().cloned() {
            let worker_cancel = cancel.child_token();
            let parent_cancel = parent_cancel.clone();
            let private_key_path = private_key_path.clone();

            waiters.push(async move {
                let task_media_name = media_name.clone();
                let result = self
                    .task(
                        format!("Wait for media {media_name} to come online"),
                        move |online| async move {
                            let load = async {
                                let media = db.lookup_media(&task_media_name).ok_or::<String>(
                                    format!("Media {task_media_name} not found in db").into(),
                                )?;
                                let backend = media.id.open_backend().await?;
                                backend.wait_for_available().await?;

                                let manifest = OpenManifest::new(backend.clone()).await?;
                                match manifest.read_verified(db, &private_key_path).await? {
                                    None => {
                                        online
                                            .set_message(format!(
                                                "Media {} does not contain this key",
                                                media.id
                                            ))
                                            .await;
                                        Ok(None)
                                    }
                                    Some(bytes) => Ok(Some((bytes, backend))),
                                }
                            };

                            tokio::select! {
                                _ = parent_cancel.cancelled() => {
                                    online.mark_cancelled("Key loading was interrupted".into()).await;
                                    Ok(None)
                                }
                                _ = worker_cancel.cancelled() => {
                                    online.mark_cancelled("Another copy was loaded".into()).await;
                                    Ok(None)
                                }
                                result = load => result,
                            }
                        },
                    )
                    .await;

                result.map(|loaded| loaded.map(|(bytes, backend)| (media_name, bytes, backend)))
            });
        }

        let mut last_error = None;
        while let Some(result) = waiters.next().await {
            match result {
                Ok(Some((media, bytes, backend))) => {
                    cancel.cancel();

                    // Poll the losing workers so they observe cancellation and
                    // their UI tasks reach a terminal state.
                    while waiters.next().await.is_some() {}

                    self.set_message(format!("Read private key from {media}"))
                        .await;
                    return match PKey::private_key_from_pem(bytes.expose_secret()) {
                        Ok(private_key) => Ok(LoadedPrivateKey {
                            private_key,
                            media,
                            backend,
                        }),
                        Err(error) => {
                            self.mark_error(format!(
                                "The key was verified from {media}, but OpenSSL could not read it: {error}"
                            ))
                            .await;
                            Err("Could not read private key from media".into())
                        }
                    };
                }
                Ok(None) => {}
                Err(error) => last_error = Some(error),
            }
        }

        if parent_cancel.is_cancelled() {
            return Err("Key loading was interrupted".into());
        }

        Err(last_error.unwrap_or_else(|| "No complete key copy could be loaded".into()))
    }
}

#[async_trait(?Send)]
impl<T: Task> UiKeypairExt for T {}
