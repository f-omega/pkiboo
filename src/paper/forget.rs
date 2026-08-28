use crate::{pkiboo::Paper, util::Name};
use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the paper artifact
    #[arg(long)]
    paper: Name<Paper>,
    /// Confirm that the artifact should no longer count toward recovery
    #[arg(long)]
    confirm: bool,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    if !args.confirm {
        return Err("Refusing to forget paper without --confirm".into());
    }
    let mut db = boo.open_database()?;
    let paper = db
        .lookup_paper(&args.paper)
        .ok_or_else(|| format!("Paper {} not found", args.paper))?
        .clone();
    if db.share_copy_count(&paper.split, paper.share) == 1 {
        crate::cli_common::warn(format!(
            "Paper {} is the final recorded copy of share {} in split {}",
            paper.name, paper.share.0, paper.split
        ));
    }
    db.transaction().forget_paper(&paper.name);
    Ok(())
}
