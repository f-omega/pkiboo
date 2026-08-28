use crate::ui::TaskStarterExt;
use std::{error::Error, path::PathBuf};

#[derive(clap::Args)]
pub struct Args {
    /// Directory watched for images containing numbered paper-share QR chunks
    #[arg(long, value_name = "DIR")]
    paper_input: PathBuf,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let paper = boo
        .ui()
        .task("Wait for a complete paper share".into(), async |task| {
            let mut input = super::input::PaperInput::new(&args.paper_input);
            input.next_share(&task).await
        })
        .await?;

    let record = db.lookup_paper(&paper.paper_name).ok_or_else(|| {
        format!(
            "Paper {} is valid but is not registered in this database",
            paper.paper_name
        )
    })?;
    if record.share.0 != u32::from(paper.share.x) {
        return Err(format!(
            "Paper {} does not match its database record",
            paper.paper_name
        )
        .into());
    }
    let split = db
        .lookup_split(&record.split)
        .ok_or_else(|| format!("Share set {} not found", record.split))?;
    if split.key != record.key {
        return Err(format!(
            "Paper {} belongs to key {}, but share set {} belongs to key {}",
            paper.paper_name, record.key, split.label, split.key
        )
        .into());
    }
    if split.num_splits != u32::from(paper.share.shamir.shares)
        || split.min_splits != u32::from(paper.share.shamir.threshold)
    {
        crate::cli_common::warn(format!(
            "Paper {} is cryptographically valid, but its Shamir parameters ({}/{}) differ from share set {} in the database ({}/{})",
            paper.paper_name,
            paper.share.shamir.threshold,
            paper.share.shamir.shares,
            split.label,
            split.min_splits,
            split.num_splits
        ));
    }
    let key = db
        .lookup_key(&record.key)
        .ok_or_else(|| format!("Key {} not found", record.key))?;
    let fingerprint =
        crate::multihash::MultiHash::with_default_algo(&key.public_key.as_bytes().to_vec());
    if paper.share.public_key != fingerprint {
        return Err(format!(
            "Paper {} does not identify managed key {}",
            paper.paper_name, key.name
        )
        .into());
    }
    eprintln!(
        "Paper {} is a valid share {} of {} for key {}.",
        paper.paper_name, paper.share.x, paper.share.shamir.shares, key.name
    );
    Ok(())
}
