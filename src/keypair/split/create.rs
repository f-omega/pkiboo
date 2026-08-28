use crate::{
    media::OpenManifest,
    pkiboo::{Db, Key, Media, Meta, Paper, ShareNumber, Split, SplitBackup},
    ui::{TaskStarterExt, UiKeypairExt},
    util::Name,
};
use futures::{StreamExt, stream::FuturesUnordered};
use secrecy::SecretBox;
use std::{
    collections::HashSet,
    error::Error,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(clap::Args)]
pub struct Args {
    /// Key to split
    #[arg(long)]
    key: String,
    /// Shares required to reconstruct the key
    #[arg(long)]
    threshold: usize,
    /// Total number of shares to create
    #[arg(long)]
    shares: usize,
    /// Registered media on which to place one share (repeat for each medium)
    #[arg(long, value_name = "MEDIA")]
    media: Vec<String>,
    /// Issue every share not assigned to media as a paper share
    ///
    /// Paper names are generated from six random words. A paper share may use
    /// multiple QR codes; each payload will carry its sequence number and total
    /// so scans can be assembled in any order.
    #[arg(long)]
    paper: bool,

    /// Directory in which generated paper-share PDFs are created
    #[arg(long, value_name = "DIR", default_value = ".")]
    paper_output_dir: PathBuf,

    /// Filename prefix; produces PREFIX-SHARE.pdf instead of PAPER-NAME.pdf
    #[arg(long, value_name = "PREFIX")]
    paper_output_prefix: Option<String>,

    /// Allow different shares to be placed on the same medium
    #[arg(long)]
    allow_duplicate: bool,
}

struct Destinations {
    media: Vec<Name<Media>>,
    paper: Vec<Name<Paper>>,
}

impl Args {
    fn destinations(&self, db: &Db) -> Result<Destinations, Box<dyn Error>> {
        let key_name = Name::<Key>::new(self.key.clone());
        let key = db
            .lookup_key(&key_name)
            .ok_or_else(|| format!("could not find key \"{}\"", self.key))?;

        if self.media.len() > self.shares {
            return Err(format!(
                "requested {} shares, but supplied {} media destinations",
                self.shares,
                self.media.len()
            )
            .into());
        }
        if !self.paper && self.media.len() != self.shares {
            return Err(format!(
                "requested {} shares, but supplied {} media destinations; pass --paper to issue the remaining {} shares on paper",
                self.shares,
                self.media.len(),
                self.shares - self.media.len()
            )
            .into());
        }

        let mut media_names = HashSet::new();
        for name in &self.media {
            if !media_names.insert(name.as_str()) && !self.allow_duplicate {
                return Err(format!(
                    "media {name:?} was selected more than once; every share destination must be unique"
                )
                .into());
            }
            if key.backups.contains(&Name::new(name.clone())) {
                return Err(format!(
                    "media {name:?} already contains a complete copy of key {:?}; placing a share there would not provide independent redundancy",
                    self.key
                )
                .into());
            }
        }
        let media = self
            .media
            .iter()
            .map(|name| {
                let name = Name::<Media>::new(name.clone());
                db.lookup_media(&name).ok_or_else(|| -> Box<dyn Error> {
                    format!("could not find registered media \"{name}\"").into()
                })?;
                Ok(name)
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

        let paper_count = if self.paper {
            self.shares - self.media.len()
        } else {
            0
        };
        let mut paper = Vec::with_capacity(paper_count);
        let mut generated_names = HashSet::new();
        while paper.len() < paper_count {
            let generated =
                petname::petname(6, "-").ok_or("could not generate a random paper name")?;
            if generated_names.insert(generated.clone()) {
                let name = Name::<Paper>::new(generated);
                if db.lookup_paper(&name).is_none() {
                    paper.push(name);
                }
            }
        }

        Ok(Destinations { media, paper })
    }
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _split: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let destinations = args.destinations(&db)?;
    let split_name = generate_split_name(&db)?;
    let key_name = Name::<Key>::new(args.key.clone());
    let key = db
        .lookup_key(&key_name)
        .expect("destinations validated key")
        .clone();
    let source = boo
        .ui()
        .task("Load private key for splitting".into(), async |task| {
            task.load_private_key_with_source(&db, &key_name).await
        })
        .await?;
    let private_pem = source.private_key.private_key_to_pem_pkcs8()?;
    let public_pem = key.public_key.as_bytes().to_vec();
    let fingerprint = crate::multihash::MultiHash::with_default_algo(&public_pem);
    let share_files = super::share::split_private_key(
        &private_pem,
        fingerprint,
        u8::try_from(args.shares)?,
        u8::try_from(args.threshold)?,
    )?;

    // Mandatory ceremony self-test: verify every generated share, reconstruct
    // from the complete set, decrypt, and compare exact input bytes before I/O.
    let recovered = super::share::recover_private_key(&share_files)?;
    if recovered != private_pem {
        return Err(
            "CEREMONY CHECK FAILED: reconstructed private-key bytes differ; no shares were written"
                .into(),
        );
    }
    eprintln!(
        "CHECK PASSED: all generated shares verified and reconstructed private-key PEM bytes match exactly"
    );

    let storage_locations = destinations
        .media
        .iter()
        .enumerate()
        .map(|(index, name)| format!("share {}: {}", index + 1, name))
        .collect::<Vec<_>>();
    let paper_locations = destinations
        .paper
        .iter()
        .enumerate()
        .map(|(index, name)| format!("share {}: {}", destinations.media.len() + index + 1, name))
        .collect::<Vec<_>>();
    let paper_placements = super::share::PaperSharePlacements {
        paper: paper_locations,
        storage: storage_locations,
    };
    let mut rendered_papers = Vec::with_capacity(destinations.paper.len());
    if let Some(prefix) = &args.paper_output_prefix {
        validate_paper_output_prefix(prefix)?;
    }
    for (offset, paper_name) in destinations.paper.iter().cloned().enumerate() {
        let share_index = destinations.media.len() + offset;
        let share_number = ShareNumber((share_index + 1) as u32);
        let paper_share = super::share::PaperShare {
            key_name: key_name.to_string(),
            paper_name: paper_name.clone(),
            share: share_files[share_index].clone(),
            placements: paper_placements.clone(),
        };
        let filename = paper_output_filename(
            &paper_name,
            share_number,
            args.paper_output_prefix.as_deref(),
        );
        let path = args.paper_output_dir.join(filename);
        if path.exists() {
            return Err(format!(
                "refusing to replace existing paper-share PDF {}",
                path.display()
            )
            .into());
        }
        rendered_papers.push((
            paper_name,
            share_number,
            path,
            crate::paper::pdf::generate_paper_pdf(&paper_share)?,
        ));
    }

    // The full-key medium cannot be a share destination. Release it before
    // concurrently waiting for the destination media.
    source.backend.release().await?;

    let signing_key = super::super::LoadedKey::new(
        super::super::PrivateKey {
            algo: key.algorithm.clone(),
            pkey: source.private_key.clone(),
        },
        key.clone(),
    );
    let mut writers = FuturesUnordered::new();
    for (index, media_name) in destinations.media.iter().cloned().enumerate() {
        let media = db
            .lookup_media(&media_name)
            .expect("validated media")
            .clone();
        let yaml = yaml_serde::to_string(&share_files[index])?.into_bytes();
        let path = Split {
            label: split_name.clone(),
            key: key_name.clone(),
            num_splits: args.shares as u32,
            min_splits: args.threshold as u32,
            meta: Meta::new(),
            backups: vec![],
            verifications: vec![],
        }
        .share(ShareNumber((index + 1) as u32))
        .path();
        let signing_key = &signing_key;
        writers.push(async move {
            let backend = media.id.open_backend().await?;
            backend.wait_for_available().await?;
            backend.setup().await?;
            let mut manifest = OpenManifest::new(backend.clone()).await?;
            manifest
                .write_file(path, signing_key, SecretBox::new(Box::new(yaml)))
                .await?;
            manifest.save().await?;
            Ok::<_, Box<dyn Error>>((media.label, ShareNumber((index + 1) as u32), backend))
        });
    }
    let mut placements = Vec::new();
    while let Some(result) = writers.next().await {
        let (media, share, backend) = result?;
        placements.push(SplitBackup { share, media });
        if let Err(error) = db.write_recovery_hint(backend.clone()).await {
            crate::cli_common::warn(format!(
                "Share was written, but its recovery hint was not refreshed: {error}"
            ));
        }
        backend.release().await?;
    }
    let mut paper_records = Vec::with_capacity(rendered_papers.len());
    for (name, share, path, pdf) in rendered_papers {
        write_new_private_file(&path, &pdf)?;
        eprintln!(
            "PAPER ISSUED: share {} written to {}",
            share.0,
            path.display()
        );
        paper_records.push(Paper {
            name,
            key: key_name.clone(),
            split: split_name.clone(),
            share,
            meta: Meta::new(),
        });
    }
    if !paper_records.is_empty() {
        crate::cli_common::warn(
            "Paper shares have been issued as PDF files. Print every PDF and then delete the PDF files; leaving them on disk creates additional copies of the shares.".into(),
        );
    }
    let mut tx = db.transaction();
    tx.add_split(Split {
        label: split_name.clone(),
        key: key_name,
        num_splits: args.shares as u32,
        min_splits: args.threshold as u32,
        meta: Meta::new(),
        backups: placements,
        verifications: vec![],
    });
    for paper in paper_records {
        tx.add_paper(paper);
    }
    drop(tx);
    eprintln!("Split {split_name} created.");
    Ok(())
}

fn write_new_private_file(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn generate_split_name(db: &Db) -> Result<Name<Split>, Box<dyn Error>> {
    loop {
        let generated = petname::petname(6, "-").ok_or("could not generate a random split name")?;
        let name = Name::<Split>::new(generated);
        if db.lookup_split(&name).is_none() {
            return Ok(name);
        }
    }
}

fn validate_paper_output_prefix(prefix: &str) -> Result<(), Box<dyn Error>> {
    if prefix.is_empty()
        || prefix == "."
        || prefix == ".."
        || Path::new(prefix).components().count() != 1
        || prefix.contains(['/', '\\'])
    {
        return Err("--paper-output-prefix must be one non-empty filename component".into());
    }
    Ok(())
}

fn paper_output_filename(
    paper_name: &Name<Paper>,
    share: ShareNumber,
    prefix: Option<&str>,
) -> String {
    prefix
        .map(|prefix| format!("{prefix}-{}.pdf", share.0))
        .unwrap_or_else(|| format!("{paper_name}.pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::{Algorithm, Ed25519Spec};

    fn db() -> Db {
        let mut db = Db::empty();
        db.keys.push(Key::new(
            Name::new("root".into()),
            Algorithm::ED25519(Ed25519Spec {}),
            "public key PEM".into(),
        ));
        db
    }

    fn args(shares: usize, media: &[&str], paper: bool) -> Args {
        Args {
            key: "root".into(),
            threshold: 2,
            shares,
            media: media.iter().map(|name| (*name).into()).collect(),
            paper,
            allow_duplicate: false,
            paper_output_dir: PathBuf::from("."),
            paper_output_prefix: None,
        }
    }

    #[test]
    fn destination_count_must_equal_share_count() {
        let error = args(3, &[], false)
            .destinations(&db())
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("pass --paper to issue the remaining 3 shares"));
    }

    #[test]
    fn media_names_must_be_unique() {
        let error = args(2, &["vault", "vault"], false)
            .destinations(&db())
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("selected more than once"));
    }

    #[test]
    fn media_names_must_exist() {
        let error = args(1, &["missing"], false)
            .destinations(&db())
            .err()
            .unwrap()
            .to_string();
        assert_eq!(error, "could not find registered media \"missing\"");
    }

    #[test]
    fn media_with_a_complete_key_copy_is_rejected() {
        let mut db = db();
        db.keys[0].add_backup(Name::new("vault".into()));

        let error = args(1, &["vault"], false)
            .destinations(&db)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("already contains a complete copy"));
        assert!(error.contains("would not provide independent redundancy"));
    }

    #[test]
    fn paper_flag_fills_all_unassigned_destinations() {
        let destinations = args(2, &[], true).destinations(&db()).unwrap();
        assert_eq!(destinations.paper.len(), 2);
        assert!(destinations.paper[0] != destinations.paper[1]);
        for name in destinations.paper {
            assert_eq!(name.split('-').count(), 6);
        }
    }

    #[test]
    fn paper_filename_uses_name_without_prefix() {
        assert_eq!(
            paper_output_filename(
                &Name::new("six-word-paper-name-here-now".into()),
                ShareNumber(4),
                None
            ),
            "six-word-paper-name-here-now.pdf"
        );
    }

    #[test]
    fn paper_filename_uses_prefix_and_share_number() {
        assert_eq!(
            paper_output_filename(
                &Name::new("ignored".into()),
                ShareNumber(4),
                Some("root-recovery")
            ),
            "root-recovery-4.pdf"
        );
        assert!(validate_paper_output_prefix("../escape").is_err());
    }
}
