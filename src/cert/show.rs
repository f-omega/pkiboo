use crate::pkiboo::Cert;
use crate::ui::{PaneStarterExt, Presenter, Property, PropertyList, PropertyListView};
use crate::util::Name;
use openssl::hash::MessageDigest;
use openssl::x509::{X509, X509NameRef};
use std::error::Error;
use std::io::Write;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the certificate
    #[arg(long)]
    cert: Name<Cert>,

    /// Print only the PEM-encoded public certificate
    #[arg(long)]
    pem: bool,
}

fn display_name(name: &X509NameRef) -> Result<String, Box<dyn Error>> {
    name.entries()
        .map(|entry| {
            let field = entry.object().nid().short_name().unwrap_or("unknown");
            Ok(format!("{field}={}", entry.data().to_string()?))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()
        .map(|entries| entries.join(", "))
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _cert: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let cert = db
        .lookup_cert(&args.cert)
        .ok_or_else(|| format!("Certificate {} not found", args.cert))?;

    if args.pem {
        std::io::stdout()
            .lock()
            .write_all(cert.certificate.as_bytes())?;
        return Ok(());
    }

    let certificate = X509::from_pem(cert.certificate.as_bytes())?;
    let fingerprint = certificate
        .digest(MessageDigest::sha256())?
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    let serial = certificate
        .serial_number()
        .to_bn()?
        .to_hex_str()?
        .to_string();

    boo.ui()
        .pane(
            "Certificate details".into(),
            async |pane| -> Result<(), Box<dyn Error>> {
                pane.property_list(PropertyList::new([
                    Property::new("Name", cert.name.to_string()),
                    Property::new("Key", cert.key.to_string()),
                    Property::new(
                        "Issuer certificate",
                        cert.issuer
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "self-signed".into()),
                    ),
                    Property::new("Subject", display_name(certificate.subject_name())?),
                    Property::new("Issuer", display_name(certificate.issuer_name())?),
                    Property::new("Serial", serial),
                    Property::new("SHA-256 fingerprint", fingerprint),
                    Property::new("Not before", certificate.not_before().to_string()),
                    Property::new("Not after", certificate.not_after().to_string()),
                ]))
                .display()
                .await;
                Ok(())
            },
        )
        .await?;

    Ok(())
}
