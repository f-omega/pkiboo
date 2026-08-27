use futures::future::try_join_all;
use std::error::Error;
use openssl::pkey::{PKey, Private};
use crate::{media::OpenManifest, pkiboo::{Key, OpenedDb}, util::Name};
use super::Task;

pub trait UiKeypairExt : Task {

    /// Load a private key from any media it's available on
    async fn load_private_key(&self, db: &OpenedDb, key_id: &Name<Key>) -> Result<PKey<Private>, Box<dyn Error>> {
        let key = db.lookup_key(key_id).ok_or("Could not find key".into())?;
        let cancel = tokio_util::sync::CancellationToken::new();
        let (send_key, mut rx_key) = tokio::sync::watch::channel(None);

        let private_key_path = key.key_path();
        for nm in key.backups {
            tokio::spawn(async {
                self.task(format!("Wait for media {nm} to come online").into(),
                          async |online| {
                              let media = db.lookup_media(&nm)?;
                              let backend = media.id.open_backend().await?;
                              backend.wait_for_available().await?;

                              // Once available, we can open the manifest, verify and read the key
                              let manifest = OpenManifest::new(backend.clone()).await?;

                              match manifest.read_verified(&db, key_path).await? {
                                  None => online.set_message(format!("Media {} does not seem to contain this key", media.id).into()),
                                  Some(bytes) => {
                                      // Read private key from PEM
                                      match openssl::pkey::PKey::private_key_from_pem(bytes.expose_secret()) {
                                          Err(_) => online.set_message(format!("The key was successfully read and verified from {}, but OpenSSL could not read it", media.id).into()),
                                          Ok(key) => send_key.send(Some(key)).await?
                                      }
                                  }
                              };
                              Ok(())
                          }).await
            })
        };

        match rx_key.changed().await {
            Err(_) => return Err::<PKey<Private>, Box<dyn Error>>("No key was able to be retrieved".into()),
            Ok(_) => Ok(rx_key.borrow_and_update().unwrap())
        }
    }
}
