use crate::{
    media::OpenManifest,
    pkiboo::{Media, Paper, ShareNumber, Split, SplitBackup, SplitVerification},
    ui::{Task, TaskStarterExt},
    util::Name,
};
use futures::{StreamExt, stream::FuturesUnordered};
use openssl::pkey::{PKey, Private};
use secrecy::ExposeSecret;
use std::{
    collections::{HashMap, HashSet},
    error::Error,
};

#[derive(clap::Args)]
pub struct Args {
    /// Recovery share set to verify
    #[arg(long)]
    split: Name<Split>,

    /// Verify shares recorded on this medium; repeat as needed
    #[arg(long, value_name = "MEDIA")]
    media: Vec<Name<Media>>,

    /// Verify this paper share; repeat as needed (not yet implemented)
    #[arg(long, value_name = "PAPER")]
    paper: Vec<Name<Paper>>,

    /// Require enough verified distinct shares to reconstruct and check the key
    #[arg(long)]
    reconstruction: bool,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _share: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let split = db
        .lookup_split(&args.split)
        .ok_or_else(|| format!("Share set {} not found", args.split))?
        .clone();
    let key = db
        .lookup_key(&split.key)
        .ok_or_else(|| format!("Key {} not found", split.key))?
        .clone();
    if !args.paper.is_empty() {
        for paper in &args.paper {
            if db.lookup_paper(paper).is_none() {
                return Err(format!("Paper {paper} not found").into());
            }
        }
        return Err(
            "Paper-share verification requires QR prompting and scanning, which is not implemented"
                .into(),
        );
    }

    let selected = select_media(&db, &split, &args.media)?;
    let fingerprint =
        crate::multihash::MultiHash::with_default_algo(&key.public_key.as_bytes().to_vec());
    let mut workers = selected
        .into_iter()
        .map(|(media, numbers)| {
            let db = &db;
            let split = &split;
            let fingerprint = fingerprint.clone();
            async move {
                let label = media.label.clone();
                let result = boo
                    .ui()
                    .task(format!("Verify shares on {}", media.label), async |task| {
                        task.set_message(format!("Insert media {}", media.label))
                            .await;
                        let backend = media.id.open_backend().await?;
                        let verify = async {
                            backend.wait_for_available().await?;
                            let manifest = OpenManifest::new(backend.clone()).await?;
                            let mut found = Vec::new();
                            for number in numbers {
                                let path = split.share(number).path();
                                let bytes =
                                    manifest.read_verified(db, &path).await?.ok_or_else(|| {
                                        format!(
                                            "Media {} is missing share {}",
                                            media.label, number.0
                                        )
                                    })?;
                                let share = super::reconstruct::load_share(
                                    bytes.expose_secret(),
                                    number,
                                    split,
                                    &fingerprint,
                                )?;
                                found.push((number, share));
                            }
                            Ok::<_, Box<dyn Error>>(found)
                        }
                        .await;
                        let release = backend.release().await;
                        match (verify, release) {
                            (Ok(found), Ok(_)) => Ok(found),
                            (Err(error), _) => Err(error),
                            (_, Err(error)) => Err(error),
                        }
                    })
                    .await;
                result.map(|shares| (label, shares))
            }
        })
        .collect::<FuturesUnordered<_>>();

    let mut verified = HashMap::new();
    let mut placements = HashMap::new();
    let mut failures = Vec::new();
    while let Some(result) = workers.next().await {
        match result {
            Ok((media, shares)) => {
                for (number, share) in shares {
                    verified.entry(number).or_insert(share);
                    placements.entry(number).or_insert(media.clone());
                }
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    drop(workers);
    if !failures.is_empty() {
        return Err(format!(
            "One or more media did not provide valid shares: {}",
            failures.join("; ")
        )
        .into());
    }
    eprintln!(
        "Verified {} distinct share{} for {}.",
        verified.len(),
        if verified.len() == 1 { "" } else { "s" },
        split.label
    );

    if verified.len() >= split.min_splits as usize {
        let chosen = verified
            .iter()
            .take(split.min_splits as usize)
            .map(|(number, share)| (*number, share.clone()))
            .collect::<Vec<_>>();
        let private_pem = super::share::recover_private_key(
            &chosen.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>(),
        )?;
        let private_key: PKey<Private> = PKey::private_key_from_pem(&private_pem)?;
        let expected = key.load_public_key()?;
        if !private_key.public_eq(&expected) {
            return Err("Reconstructed private key does not match the managed public key".into());
        }
        let evidence = chosen
            .iter()
            .map(|(number, _)| SplitBackup {
                share: *number,
                media: placements[number].clone(),
            })
            .collect::<Vec<_>>();
        let mut tx = db.transaction();
        tx.splits
            .iter_mut()
            .find(|candidate| candidate.label == split.label)
            .expect("validated split disappeared")
            .verifications
            .push(SplitVerification {
                verified_at: chrono::Utc::now(),
                shares: evidence,
            });
        drop(tx);
        eprintln!("Reconstruction succeeded and matches key {}.", key.name);
    } else if args.reconstruction {
        let has_paper = db.papers.iter().any(|paper| paper.split == split.label);
        return Err(if has_paper {
            format!("Reconstruction requires {} distinct shares; paper-share prompting and QR scanning are not implemented",split.min_splits)
        } else {
            format!("Reconstruction requires {} distinct shares, but only {} valid media shares were selected",split.min_splits,verified.len())
        }.into());
    }
    Ok(())
}

fn select_media(
    db: &crate::pkiboo::Db,
    split: &Split,
    requested: &[Name<Media>],
) -> Result<Vec<(Media, Vec<ShareNumber>)>, Box<dyn Error>> {
    let names = if requested.is_empty() {
        split
            .backups
            .iter()
            .map(|backup| backup.media.clone())
            .collect::<Vec<_>>()
    } else {
        requested.to_vec()
    };
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    for name in names {
        if !seen.insert(name.to_string()) {
            continue;
        }
        let media = db
            .lookup_media(&name)
            .ok_or_else(|| format!("Media {name} not found"))?
            .clone();
        let numbers = split
            .backups
            .iter()
            .filter(|backup| backup.media == name)
            .map(|backup| backup.share)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if numbers.is_empty() {
            return Err(format!("Media {name} has no recorded shares for {}", split.label).into());
        }
        selected.push((media, numbers));
    }
    if selected.is_empty() {
        return Err(format!("Share set {} has no media copies to verify", split.label).into());
    }
    Ok(selected)
}
