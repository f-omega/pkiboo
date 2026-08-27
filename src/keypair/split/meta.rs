use crate::util::Name;
use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the recovery split
    #[arg(long)]
    split: Name<crate::pkiboo::Split>,

    #[command(flatten)]
    meta: crate::pkiboo::MetaSetArgs,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _split: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let mut split = db
        .lookup_split(&args.split)
        .ok_or_else(|| format!("Split {} not found", args.split))?
        .clone();

    let mut tx = db.transaction();
    split.meta.manage(boo.ui(), &args.meta).await;
    tx.update_split(split)?;
    Ok(())
}
