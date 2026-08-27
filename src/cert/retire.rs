use std::error::Error;

use crate::pkiboo::Cert;
use crate::util::Name;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the certificate
    #[arg(long)]
    cert: Name<Cert>,

    /// Reason for retiring the certificate
    #[arg(long)]
    reason: Option<String>,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _cert: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let mut certificate = db
        .lookup_cert(&args.cert)
        .ok_or_else(|| format!("Certificate {} not found", args.cert))?
        .clone();

    if let Some(retired_at) = certificate.retired_at {
        return Err(format!(
            "Certificate {} was already retired at {}",
            certificate.name,
            retired_at.to_rfc3339()
        )
        .into());
    }

    certificate.retired_at = Some(chrono::Utc::now());
    certificate.retirement_reason = args.reason.clone();
    db.transaction().update_cert(certificate)?;
    Ok(())
}
