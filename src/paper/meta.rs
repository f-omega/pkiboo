use crate::{pkiboo::Paper, util::Name};
use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the paper artifact
    #[arg(long)]
    paper: Name<Paper>,

    #[command(flatten)]
    meta: crate::pkiboo::MetaSetArgs,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let mut paper = db
        .lookup_paper(&args.paper)
        .ok_or_else(|| format!("Paper {} not found", args.paper))?
        .clone();
    paper.meta.manage(boo.ui(), &args.meta).await;
    db.transaction().update_paper(paper)?;
    Ok(())
}
