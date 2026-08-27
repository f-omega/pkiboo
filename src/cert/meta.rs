use crate::util::Name;
use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the certificate
    cert: Name<crate::pkiboo::Cert>,

    #[command(flatten)]
    meta: crate::pkiboo::MetaSetArgs,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _cert: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let mut cert = db
        .lookup_cert(&args.cert)
        .ok_or_else(|| format!("Certificate {} not found", args.cert))?
        .clone();

    let mut tx = db.transaction();
    cert.meta.manage(boo.ui(), &args.meta).await;
    tx.update_cert(cert)?;
    Ok(())
}
