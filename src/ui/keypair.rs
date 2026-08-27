use secrecy::ExposeSecret;
use futures::future::try_join_all;
use std::error::Error;
use openssl::pkey::{PKey, Private};
use crate::{media::OpenManifest, pkiboo::{Key, OpenedDb}, util::Name};
use super::{Task, TaskStarterExt};

pub trait UiKeypairExt : Task {

    /// Load a private key from any media it's available on
    async fn load_private_key(&self, db: &OpenedDb, key_id: &Name<Key>) -> Result<PKey<Private>, Box<dyn Error>> {
        let key = db.lookup_key(key_id).ok_or::<String>("Could not find key".into())?;
        let cancel = tokio_util::sync::CancellationToken::new();
        let (send_key, mut rx_key) = tokio::sync::watch::channel(None);

        let private_key_path = key.key_path();
        let mut waiters = Vec::new();
        for nm in &key.backups {
            let handle = tokio::spawn(async {
                let _ =
                    self.task(format!("Wait for media {nm} to come online").into(),
                          async |online| {
                              let media = db.lookup_media(&nm).ok_or::<String>(format!("Media {} not found in db", nm).into())?;
                              let backend = media.id.open_backend().await?;
                              backend.wait_for_available().await?;

                              // Once available, we can open the manifest, verify and read the key
                              let manifest = OpenManifest::new(backend.clone()).await?;

                              match manifest.read_verified(&db, &private_key_path).await? {
                                  None => online.set_message(format!("Media {} does not seem to contain this key", media.id).into()).await,
                                  Some(bytes) => send_key.send(Some((nm, bytes)))?
                              };
                              Ok(())
                          }).await;
            });
            waiters.push(handle);
        };

        match rx_key.changed().await {
            Err(_) => return Err::<PKey<Private>, Box<dyn Error>>("No key was able to be retrieved".into()),
            Ok(_) => {
                let (media, bytes) = rx_key.borrow_and_update().unwrap();
                self.set_message(format!("Read private key from {}", media).into()).await;
                // Read private key from PEM
                match openssl::pkey::PKey::private_key_from_pem(bytes.expose_secret()) {
                    Err(_) => {
                        self.mark_error(format!("The key was successfully read and verified from {}, but OpenSSL could not read it", media).into()).await;
                        Err("Could not read private key from media".into())
                    },
                    Ok(key) => Ok(key)
                }
            }
        }
    }
}
