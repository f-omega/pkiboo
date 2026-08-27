use std::error::Error;
use openssl::pkey::{PKey, Public};
use crate::util::Name;
use crate::cli_common::Duration;
use crate::pkiboo::{Key, Root};
use crate::ui::UiExt;

#[derive(clap::Parser)]
pub struct Args {
    /// Key to sign with
    #[arg(long)]
    key: Option<Name<Key>>,

    /// CSR file to use instead of command lines or interactive questioning
    #[arg(long)]
    csr: Option<String>,

    // Cert creation options

    /// Issuing certificate to use
    #[arg(long)]
    by: Option<Name<Root>>,


    /// Desired certificate validity
    #[arg(long, default_value="1y")]
    validity: Duration,

    /// Common Name (CN)
    #[arg(long)]
    common_name: Option<String>,

    /// Organization (O)
    #[arg(long)]
    organization: Option<String>,

    /// Organizational Unit (OU)
    #[arg(long)]
    organizational_unit: Option<String>,

    /// Country (C)
    #[arg(long)]
    country: Option<String>,

    /// State (ST)
    #[arg(long)]
    state: Option<String>,

    /// Locality (L)
    #[arg(long)]
    locality: Option<String>
}

impl Args {
    fn make_csr(&self, public_key: &PKey<Public>)
                -> Result<Option<openssl::x509::X509Req>, Box<dyn Error>> {
        use openssl::x509::extension::{BasicConstraints,
                                       KeyUsage};
        let name = self.make_x509_name()?;
        if name.entries().count() == 0 { return Ok(None); } // No name was given

        let mut builder = openssl::x509::X509ReqBuilder::new()?;
        builder.set_version(0)?; // X509 v3
        builder.set_subject_name(&name)?;
        builder.set_pubkey(public_key)?;

        let mut extensions = openssl::stack::Stack::new()?;
        extensions.push(BasicConstraints::new().critical().ca().build()?)?;
        extensions.push(KeyUsage::new().critical()
                        .key_cert_sign()
                        .crl_sign()
                        .build()?)?;

        builder.add_extensions(&extensions)?;
        Ok(Some(builder.build()))
    }

    fn make_x509_name(&self) -> Result<openssl::x509::X509Name, Box<dyn Error>> {
        let mut builder = openssl::x509::X509NameBuilder::new()?;
        if let Some(cn) = &self.common_name {
            builder.append_entry_by_text("CN", &cn)?;
        };
        if let Some(o) = &self.organization {
            builder.append_entry_by_text("O", &o)?;
        };
        if let Some(ou) = &self.organizational_unit {
            builder.append_entry_by_text("OU", &ou)?;
        }
        if let Some(c) = &self.country {
            builder.append_entry_by_text("C", &c)?;
        }
        if let Some(st) = &self.state {
            builder.append_entry_by_text("ST", &st)?;
        }
        if let Some(l) = &self.locality {
            builder.append_entry_by_text("L", &l)?;
        };
        Ok(builder.build())
    }
}

async fn create_csr<Ui: crate::Ui>
    (boo: &crate::pkiboo::PkiBoo<Ui>,
     args: &Args,
     task: Ui::TaskHandle,
     public_key: Option<&PKey<Public>>)
     -> Result<openssl::x509::X509Req, Box<dyn Error>>
{
    let cli_csr = match public_key {
        None => Ok(None),
        Some(key) => args.make_csr(key)
    };
    match (cli_csr, &args.csr) {
        (Ok(None), Some(csr_file)) => {
            todo!("Load csr");
        },
        (Ok(Some(_)), Some(_)) => Err("Either --csr or certificate details must be given".into()),
        (Ok(None), None) => {
            todo!("We can't yet prompt for CSR details")
        },
        (Ok(Some(csr)), None) => Ok(csr),
        (Err(e), _) => Err(e),
    }
}

pub async fn main<Ui: crate::Ui>
    (boo: &crate::pkiboo::PkiBoo<Ui>,
     cert: &super::Args,
     args: &Args) -> Result<(), Box<dyn Error>>
{
    let db = boo.open_database()?;
    let public_key =
        boo.ui().task("Find Key".into(),
                      async |task| {
                          if let Some(issuer) = &args.by {
                          };
                          if let Some(key) = &args.key {
                              let pem = db.lookup_key(key).ok_or(format!("Could not find key {key}"))?.public_key.clone();
                              return Ok(Some(openssl::pkey::PKey::public_key_from_pem(pem.as_bytes())?));
                          };
                          Ok(None)
                      }).await?;
    let csr = boo.ui().task("Create CSR".into(),
                            async |task| create_csr(boo, args, task, public_key.as_ref()).await ).await?;

    // Lookup private key coresponding to CSR
    let pkey = boo.ui().task("Load private key".into(),
                             async |task| {
                                 db.lookup_key_by_public_key
                             }).await?;

    // If this is a cert issuer, the public key must match
    
    todo!("Sign CSR");
}
