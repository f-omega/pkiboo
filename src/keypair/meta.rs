use crate::util::Name;
use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the key
    key: Name<crate::pkiboo::Key>,

    #[command(flatten)]
    meta: crate::pkiboo::MetaSetArgs,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _key: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let mut key = db
        .lookup_key(&args.key)
        .ok_or_else(|| format!("Key {} not found", args.key))?
        .clone();

    let mut tx = db.transaction();
    key.meta.manage(boo.ui(), &args.meta).await;
    tx.update_key(key)?;
    Ok(())
}
