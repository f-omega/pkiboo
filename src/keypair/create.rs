use futures::future::try_join_all;
use std::error::Error;
use itertools::Itertools;
use crate::ui::{Task, TaskStarterExt};
use crate::pkiboo::{Media, Key};
use crate::media::MediaId;
use crate::media::OpenManifest;
use crate::util::Name;

#[derive(clap::Parser)]
pub struct Args {
    #[command(flatten)]
    algo_spec: super::AlgorithmArgs,

    #[arg(long)]
    name: Name<Key>,

    /// Media to place the key on
    #[arg(long)]
    media: Vec<String>
}


pub(crate) async fn main<Ui: crate::Ui>
    (boo: &crate::PkiBoo<Ui>,
     _keypair: &super::Args,
     create: &Args) -> Result<(), Box<dyn Error>>
{
    let mut db = boo.open_database()?;

    // Choose media
    let (media, prompt_user_for_media_selection) : (Vec<Media>, bool) = if create.media.is_empty() {
        todo!("Need to get online media");
        // crate::media::get_online_media(&db).await?
    } else {
        // Lookup each media
        let m  = create.media.iter()
            .map(|m| {
                let id = crate::media::MediaRef::new(m.clone()).resolve(&db)?;
                let media = db.lookup_media_by_id(&id).ok_or::<String>(format!("Could not find media {}", id).into())?.clone();
                Ok::<Media, Box<dyn Error>>(media)
            })
            .collect::<Result<Vec<Media>, _>>()?;
        (m, false)
    };

    if !prompt_user_for_media_selection && media.is_empty() {
        return Err("No media chosen for this key".into())
    }

    let (pkey, algo) = boo.ui().task("Generating key pair".into(),
                            async |task| {
                                let algo = create.algo_spec.to_algorithm().ok_or::<String>("Invalid key algorithm specified".into())?;
                                task.set_message(format!("Creating keypair using algorithm {algo}").into()).await;
                                let key = algo.generate_key()?;
                                task.set_message(format!("Created keypair using algorithm {algo}").into()).await;
                                Ok((key, algo))
                            }).await?;

    let final_media = if prompt_user_for_media_selection {
        todo!()
    } else {
        media
    };

    let public_key_pem = String::from_utf8(pkey.pkey.public_key_to_pem()?)?;

    let loaded = {
        let mut tx = db.transaction();
        let key = Key::new(create.name.clone(),
                           algo.clone(),
                           public_key_pem);
        tx.add_key(key.clone());
        super::LoadedKey::new(pkey, key)
    };

    let untrusted = final_media.iter().filter(|m| !m.trusted).map(|m| m.id.clone()).collect::<Vec<MediaId>>();
    if !untrusted.is_empty() {
        return Err(format!("Some media were untrusted: {}", untrusted.iter().format(", ")).into()); // TODO prettier output
    }

    let written = try_join_all(final_media.iter().map(async |m| {
        boo.ui().task(format!("Waiting for {} to become available", m.id).into(),
                      async |task| -> Result<_, Box<dyn Error>> {
                          let backend = m.id.open_backend().await?;
                          backend.wait_for_available().await?;

                          let mut manifest = OpenManifest::new(backend.clone()).await?;
                          loaded.save_to_media(&mut manifest).await?;
                          manifest.save().await?;

                          task.set_message(format!("Key written to {}", m.id).into()).await;
                          Ok((m.label.clone(), chrono::Utc::now(), backend))
                      }).await
    }).collect::<Vec<_>>()).await?;

    let mut updated_key = loaded.key.clone();
    for (media, verified_at, _) in &written {
        updated_key.add_backup(media.clone());
        updated_key.record_verification(media.clone(), *verified_at);
    }
    db.transaction().update_key(updated_key)?;

    let db_ref = &db;
    let _: Vec<()> = try_join_all(written.into_iter().map(|(media, _, backend)| async move {
        if let Err(error) = db_ref.write_recovery_hint(backend.clone()).await {
            crate::cli_common::warn(format!(
                "Key was written to {media}, but its database recovery hint could not be refreshed: {error}"
            ));
        }
        backend.release().await?;
        Ok::<_, Box<dyn Error>>(())
    }).collect::<Vec<_>>()).await?;

    Ok(())
}
