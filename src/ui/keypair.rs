use futures::future::try_join_all;
use std::error::Error;
use openssl::pkey::{PKey, Private};
use crate::{pkiboo::{Key, OpenedDb}, util::Name};
use super::Task;

pub trait UiKeypairExt : Task {

    /// Load a private key from any media it's available on
    async fn load_private_key(&self, db: &OpenedDb, key_id: &Name<Key>) -> Result<PKey<Private>, Box<dyn Error>> {
        let key = db.lookup_key(key_id).ok_or("Could not find key".into())?;
        let cancel = tokio_util::sync::CancellationToken::new();
        let (send_key, mut rx_key) = tokio::sync::watch::channel(None);

        for nm in key.backups {
            tokio::spawn(async || {
                self.task(format!("Wait for media {nm} to come online").into(),
                          async |online| {
                          }).await
            })
        };

        match rx_key.changed().await {
            Err(_) => return Err::<PKey<Private>, Box<dyn Error>>("No key was able to be retrieved".into()),
            Ok(_) => Ok(Some(rx_key.borrow_and_update()))
        }
    }
}
