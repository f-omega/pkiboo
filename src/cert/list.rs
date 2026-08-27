use std::error::Error;

use crate::pkiboo::Cert;
use crate::ui::ListView;
use crate::util::Name;

#[derive(clap::Args)]
pub struct Args {
    /// Only show certificates issued by this certificate
    #[arg(long)]
    by: Option<Name<Cert>>,

    #[command(flatten)]
    list_options: crate::util::ListOptions,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _cert: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;

    let certificates = db
        .certs
        .iter()
        .filter(|certificate| {
            args.by
                .as_ref()
                .is_none_or(|issuer| certificate.issuer.as_ref() == Some(issuer))
        })
        .cloned()
        .collect::<Vec<_>>();

    boo.ui()
        .list(certificates)
        .with_options(&args.list_options)
        .display()
        .await;

    Ok(())
}
