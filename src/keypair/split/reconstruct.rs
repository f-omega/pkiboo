use crate::{
    media::OpenManifest,
    pkiboo::{Key, Media, ShareNumber, Split},
    ui::{Task, TaskStarterExt},
    util::Name,
};
use futures::{StreamExt, stream::FuturesUnordered};
use openssl::pkey::{PKey, Private};
use secrecy::ExposeSecret;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    sync::Arc,
};

#[derive(clap::Args)]
pub struct Args {
    /// Managed key to restore
    #[arg(long)]
    key: Name<Key>,
    /// Destination medium for the restored complete key
    #[arg(long)]
    to: Name<Media>,
    /// Override destination trust and existing-complete-copy checks
    #[arg(long)]
    force: bool,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let key = db
        .lookup_key(&args.key)
        .ok_or_else(|| format!("Key {} not found", args.key))?
        .clone();
    let destination = db
        .lookup_media(&args.to)
        .ok_or_else(|| format!("Media {} not found", args.to))?
        .clone();
    check_restore_destination(&key, &destination, args.force)?;
    let split = choose_media_split(&db, &args.key)?;
    let fingerprint =
        crate::multihash::MultiHash::with_default_algo(&key.public_key.as_bytes().to_vec());
    let mut by_media = BTreeMap::<String, (Media, Vec<ShareNumber>)>::new();
    for backup in &split.backups {
        let media = db
            .lookup_media(&backup.media)
            .ok_or_else(|| format!("Media {} not found", backup.media))?
            .clone();
        let entry = by_media
            .entry(media.label.to_string())
            .or_insert_with(|| (media, Vec::new()));
        if !entry.1.contains(&backup.share) {
            entry.1.push(backup.share);
        }
    }

    let cancel = tokio_util::sync::CancellationToken::new();
    let mut workers=by_media.into_values().map(|(media,numbers)|{
        let cancel=cancel.child_token(); let db=&db; let split=&split; let fingerprint=fingerprint.clone();
        async move {
            let label=media.label.clone();
            let result=boo.ui().task(format!("Wait for recovery media {}",media.label),async |task|{
                task.set_message(format!("Insert media {}",media.label)).await;
                let backend=media.id.open_backend().await?;
                let load=async {
                    backend.wait_for_available().await?;
                    let manifest=OpenManifest::new(backend.clone()).await?;
                    let mut loaded=Vec::new(); let mut errors=Vec::new();
                    for number in numbers {
                        let path=split.share(number).path();
                        match manifest.read_verified(db,&path).await {
                            Ok(Some(bytes))=>match load_share(bytes.expose_secret(),number,split,&fingerprint){Ok(s)=>loaded.push((number,s)),Err(e)=>errors.push(format!("share {}: {e}",number.0))},
                            Ok(None)=>errors.push(format!("share {} is missing",number.0)),
                            Err(e)=>errors.push(format!("share {}: {e}",number.0)),
                        }
                    }
                    if loaded.is_empty(){Err(errors.join("; ").into())}else{Ok((backend.clone(),loaded))}
                };
                tokio::select! {
                    _ = cancel.cancelled() => { let _=backend.release().await; Ok(None) }
                    result = load => match result {
                        Ok(loaded) => Ok(Some(loaded)),
                        Err(error) => { let _=backend.release().await; Err(error) }
                    }
                }
            }).await;
            result.map(|r|(label,r))
        }
    }).collect::<FuturesUnordered<_>>();

    let mut shares = HashMap::new();
    let mut opened = Vec::<Arc<dyn crate::media::backend::Media>>::new();
    let mut errors = Vec::new();
    while let Some(result) = workers.next().await {
        match result {
            Ok((_, Some((backend, loaded)))) => {
                opened.push(backend);
                for (n, s) in loaded {
                    shares.entry(n).or_insert(s);
                }
                if shares.len() >= split.min_splits as usize {
                    cancel.cancel();
                }
            }
            Ok((_, None)) => {}
            Err(e) => errors.push(e.to_string()),
        }
        if cancel.is_cancelled() {
            while let Some(result) = workers.next().await {
                if let Ok((_, Some((backend, loaded)))) = result {
                    opened.push(backend);
                    for (n, s) in loaded {
                        shares.entry(n).or_insert(s);
                    }
                }
            }
            break;
        }
    }
    for backend in &opened {
        let _ = backend.release().await;
    }
    drop(workers);
    if shares.len() < split.min_splits as usize {
        return Err(format!(
            "Could not load {} distinct media shares for split {}{}",
            split.min_splits,
            split.label,
            if errors.is_empty() {
                String::new()
            } else {
                format!(": {}", errors.join("; "))
            }
        )
        .into());
    }
    let selected = shares
        .into_values()
        .take(split.min_splits as usize)
        .collect::<Vec<_>>();
    let private_pem = super::share::recover_private_key(&selected)?;
    let private_key: PKey<Private> = PKey::private_key_from_pem(&private_pem)?;
    let expected_public_key = key.load_public_key()?;
    if !private_key.public_eq(&expected_public_key) {
        return Err("Recovered private key does not match the managed public key".into());
    }

    let loaded = super::super::LoadedKey::new(
        super::super::PrivateKey {
            algo: key.algorithm.clone(),
            pkey: private_key,
        },
        key.clone(),
    );
    let backend = destination.id.open_backend().await?;
    let write = async {
        backend.wait_for_available().await?;
        backend.setup().await?;
        let mut manifest = OpenManifest::new(backend.clone()).await?;
        if args.force {
            loaded.replace_on_media(&mut manifest).await?;
        } else {
            loaded.save_to_media(&mut manifest).await?;
        }
        manifest.save().await?;
        Ok::<_, Box<dyn Error>>(())
    }
    .await;
    if write.is_ok() {
        let mut updated = key;
        updated.add_backup(destination.label.clone());
        updated.record_verification(destination.label.clone(), chrono::Utc::now());
        db.transaction().update_key(updated)?;
        if let Err(e) = db.write_recovery_hint(backend.clone()).await {
            crate::cli_common::warn(format!(
                "Key was restored, but its recovery hint was not refreshed: {e}"
            ));
        }
    }
    let release = backend.release().await;
    write?;
    release?;
    eprintln!("Key {} restored to {}.", args.key, args.to);
    Ok(())
}

fn choose_media_split(db: &crate::pkiboo::Db, key: &Name<Key>) -> Result<Split, Box<dyn Error>> {
    let splits = db.splits_for_key(key).collect::<Vec<_>>();
    if let Some(split) = splits.iter().find(|s| {
        s.backups
            .iter()
            .map(|b| b.share)
            .collect::<HashSet<_>>()
            .len()
            >= s.min_splits as usize
    }) {
        return Ok((*split).clone());
    }
    if splits
        .iter()
        .any(|split| db.papers.iter().any(|paper| paper.split == split.label))
    {
        return Err("Restoring this key requires one or more paper shares; paper restore is not implemented".into());
    }
    Err(format!("Key {key} has no recovery split with enough recorded media shares").into())
}
pub(crate) fn load_share(
    bytes: &[u8],
    number: ShareNumber,
    split: &Split,
    fingerprint: &crate::multihash::MultiHash,
) -> Result<super::share::ShamirShareFile, Box<dyn Error>> {
    let share: super::share::ShamirShareFile = yaml_serde::from_slice(bytes)?;
    if u32::from(share.x) != number.0 {
        return Err(format!("contains share {}, expected {}", share.x, number.0).into());
    }
    if share.public_key != *fingerprint {
        return Err("public-key fingerprint does not match".into());
    }
    if u32::from(share.shamir.shares) != split.num_splits
        || u32::from(share.shamir.threshold) != split.min_splits
    {
        return Err("threshold parameters do not match the split".into());
    }
    share.verify()?;
    Ok(share)
}
fn check_restore_destination(key: &Key, media: &Media, force: bool) -> Result<(), Box<dyn Error>> {
    if !media.trusted && !force {
        return Err(format!(
            "Media {} is not trusted for complete key copies; pass --force to continue",
            media.label
        )
        .into());
    }
    if key.backups.contains(&media.label) && !force {
        return Err(format!("Media {} is already recorded as containing a complete copy of key {}; pass --force to replace it",media.label,key.name).into());
    }
    if force && (!media.trusted || key.backups.contains(&media.label)) {
        crate::cli_common::warn(format!(
            "Forcing restored key {} onto {} despite destination policy checks",
            key.name, media.label
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkiboo::{Meta, Paper, SplitBackup};

    fn split(backups: Vec<SplitBackup>) -> Split {
        Split {
            label: Name::new("recovery".into()),
            key: Name::new("root".into()),
            num_splits: 3,
            min_splits: 2,
            meta: Meta::new(),
            backups,
            verifications: Vec::new(),
        }
    }

    #[test]
    fn selects_a_split_with_distinct_media_shares() {
        let mut db = crate::pkiboo::Db::empty();
        db.splits.push(split(vec![
            SplitBackup {
                share: ShareNumber(1),
                media: Name::new("a".into()),
            },
            SplitBackup {
                share: ShareNumber(2),
                media: Name::new("b".into()),
            },
        ]));
        assert_eq!(
            choose_media_split(&db, &Name::new("root".into()))
                .unwrap()
                .label
                .to_string(),
            "recovery"
        );
    }

    #[test]
    fn reports_when_paper_is_required() {
        let mut db = crate::pkiboo::Db::empty();
        db.splits.push(split(Vec::new()));
        for number in [1, 2] {
            db.papers.push(Paper {
                name: Name::new(format!("paper-{number}")),
                key: Name::new("root".into()),
                split: Name::new("recovery".into()),
                share: ShareNumber(number),
                meta: Meta::new(),
            });
        }
        assert!(
            choose_media_split(&db, &Name::new("root".into()))
                .err()
                .unwrap()
                .to_string()
                .contains("paper restore is not implemented")
        );
    }
}
