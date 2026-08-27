use crate::cli_common::Duration;
use crate::pkiboo::{Cert, Key, Meta};
use crate::ui::{TaskStarterExt, UiKeypairExt};
use crate::util::Name;
use openssl::asn1::{Asn1Integer, Asn1Time};
use openssl::bn::{BigNum, MsbOption};
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkey::{Id, PKey, Private, Public};
use openssl::x509::extension::{BasicConstraints, KeyUsage, SubjectKeyIdentifier};
use openssl::x509::{X509Req, X509};
use std::error::Error;

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
    by: Option<Name<Cert>>,

    /// Desired certificate validity
    #[arg(long, default_value = "1y")]
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
    locality: Option<String>,
}

impl Args {
    fn make_csr(
        &self,
        public_key: &PKey<Public>,
    ) -> Result<Option<openssl::x509::X509Req>, Box<dyn Error>> {
        let name = self.make_x509_name()?;
        if name.entries().count() == 0 {
            return Ok(None);
        } // No name was given

        let mut builder = openssl::x509::X509ReqBuilder::new()?;
        builder.set_version(0)?; // X509 v3
        builder.set_subject_name(&name)?;
        builder.set_pubkey(public_key)?;

        let mut extensions = openssl::stack::Stack::new()?;
        extensions.push(BasicConstraints::new().critical().ca().build()?)?;
        extensions.push(
            KeyUsage::new()
                .critical()
                .key_cert_sign()
                .crl_sign()
                .build()?,
        )?;

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

async fn create_csr<Ui: crate::Ui>(
    _boo: &crate::pkiboo::PkiBoo<Ui>,
    args: &Args,
    _task: Ui::TaskHandle,
    public_key: Option<&PKey<Public>>,
) -> Result<openssl::x509::X509Req, Box<dyn Error>> {
    let cli_csr = match public_key {
        None => Ok(None),
        Some(key) => args.make_csr(key),
    };
    match (cli_csr, &args.csr) {
        (Ok(None), Some(csr_file)) => {
            let pem = std::fs::read(csr_file)?;
            Ok(X509Req::from_pem(&pem)?)
        }
        (Ok(Some(_)), Some(_)) => Err("Either --csr or certificate details must be given".into()),
        (Ok(None), None) => {
            todo!("We can't yet prompt for CSR details")
        }
        (Ok(Some(csr)), None) => Ok(csr),
        (Err(e), _) => Err(e),
    }
}

/// Check the parts of a CSR that are safe to decide before certificate policy
/// is applied. Requested extensions are deliberately not copied from the CSR;
/// this command currently issues CA certificates with its own fixed extensions.
fn check_csr_policy(csr: &X509Req) -> Result<(), Box<dyn Error>> {
    let public_key = csr.public_key()?;
    if !csr.verify(&public_key)? {
        return Err("CSR signature is invalid".into());
    }
    if csr.subject_name().entries().next().is_none() {
        return Err("CSR subject must not be empty".into());
    }
    Ok(())
}

fn certificate_name(csr: &X509Req) -> Result<Name<Cert>, Box<dyn Error>> {
    let common_name = csr
        .subject_name()
        .entries_by_nid(Nid::COMMONNAME)
        .next()
        .ok_or("CSR subject must contain a common name")?
        .data()
        .to_string()?;
    Ok(Name::new(common_name))
}

fn random_serial() -> Result<Asn1Integer, Box<dyn Error>> {
    let mut serial = BigNum::new()?;
    serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
    Ok(serial.to_asn1_integer()?)
}

fn signing_digest(key: &PKey<Private>) -> MessageDigest {
    match key.id() {
        Id::ED25519 | Id::ED448 => MessageDigest::null(),
        _ => MessageDigest::sha512(),
    }
}

fn issue_certificate(
    csr: &X509Req,
    issuer: Option<&X509>,
    signing_key: &PKey<Private>,
    validity_days: u32,
) -> Result<X509, Box<dyn Error>> {
    let mut builder = X509::builder()?;
    builder.set_version(2)?;
    let serial = random_serial()?;
    builder.set_serial_number(&serial)?;
    builder.set_subject_name(csr.subject_name())?;
    let issuer_name = issuer
        .map(|certificate| certificate.subject_name())
        .unwrap_or_else(|| csr.subject_name());
    builder.set_issuer_name(issuer_name)?;
    let subject_key = csr.public_key()?;
    builder.set_pubkey(&subject_key)?;
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(validity_days)?;
    builder.set_not_before(&not_before)?;
    builder.set_not_after(&not_after)?;

    builder.append_extension(BasicConstraints::new().critical().ca().build()?)?;
    builder.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()?,
    )?;
    let subject_key_identifier = SubjectKeyIdentifier::new().build(&builder.x509v3_context(
        issuer.map(|certificate| certificate.as_ref()),
        None,
    ))?;
    builder.append_extension(subject_key_identifier)?;
    builder.sign(signing_key, signing_digest(signing_key))?;
    Ok(builder.build())
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::pkiboo::PkiBoo<Ui>,
    _cert: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let public_key = boo
        .ui()
        .task("Find Key".into(), async |_task| {
            if let Some(key) = &args.key {
                let pem = db
                    .lookup_key(key)
                    .ok_or(format!("Could not find key {key}"))?
                    .public_key
                    .clone();
                return Ok(Some(openssl::pkey::PKey::public_key_from_pem(
                    pem.as_bytes(),
                )?));
            };
            Ok(None)
        })
        .await?;
    let mut csr = boo
        .ui()
        .task("Create CSR".into(), async |task| {
            create_csr(boo, args, task, public_key.as_ref()).await
        })
        .await?;

    let csr_public_key = csr.public_key()?;
    let subject_key = db
        .lookup_key_by_public_key(&csr_public_key)
        .ok_or::<String>("Could not identify the managed key requested by this CSR".into())?
        .name
        .clone();
    if let Some(requested_key) = &args.key
        && requested_key != &subject_key
    {
        return Err(format!("CSR public key does not match key {requested_key}").into());
    }

    // A locally constructed request does not yet have a signature. Sign it
    // with the subject key just as an externally produced PKCS#10 request is.
    if args.csr.is_none() {
        let private_key = boo
            .ui()
            .task("Sign CSR".into(), async |task| {
                task.load_private_key(&db, &subject_key).await
            })
            .await?;
        let mut builder = X509Req::builder()?;
        builder.set_version(csr.version())?;
        builder.set_subject_name(csr.subject_name())?;
        builder.set_pubkey(&csr_public_key)?;
        builder.sign(&private_key, signing_digest(&private_key))?;
        csr = builder.build();
    }
    check_csr_policy(&csr)?;

    let (issuer_name, issuer_certificate, signing_key_name) = match &args.by {
        Some(name) => {
            let issuer = db
                .lookup_cert(name)
                .ok_or_else(|| format!("Could not find issuing certificate {name}"))?;
            (
                Some(name.clone()),
                Some(X509::from_pem(issuer.certificate.as_bytes())?),
                issuer.key.clone(),
            )
        }
        None => (None, None, subject_key.clone()),
    };
    let signing_key = boo
        .ui()
        .task("Load signing key".into(), async |task| {
            task.load_private_key(&db, &signing_key_name).await
        })
        .await?;
    let expected_signing_key = match &issuer_certificate {
        Some(issuer) => issuer.public_key()?,
        None => csr.public_key()?,
    };
    if !signing_key.public_eq(&expected_signing_key) {
        return Err(format!(
            "Private key {signing_key_name} does not match the signing certificate"
        )
        .into());
    }
    let certificate = issue_certificate(
        &csr,
        issuer_certificate.as_ref(),
        &signing_key,
        args.validity.days(),
    )?;
    if !certificate.verify(&signing_key)? {
        return Err("Issued certificate signature could not be verified".into());
    }

    let name = certificate_name(&csr)?;
    if db.lookup_cert(&name).is_some() {
        return Err(format!("Certificate {name} already exists").into());
    }
    let pem = String::from_utf8(certificate.to_pem()?)?;
    db.transaction().add_cert(Cert {
        name,
        key: subject_key,
        issuer: issuer_name,
        certificate: pem,
        created_on: chrono::Utc::now(),
        meta: Meta::new(),
    });
    Ok(())
}
