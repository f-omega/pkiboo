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
use openssl::x509::{X509, X509Req};
use std::error::Error;

#[derive(clap::Parser)]
pub struct Args {
    /// Key to sign with
    #[arg(long)]
    key: Option<Name<Key>>,

    /// CSR file to use instead of command lines or interactive questioning
    #[arg(long)]
    csr: Option<String>,

    /// Issuing certificate to use
    #[arg(long)]
    by: Option<Name<Cert>>,

    /// Validity of the certificate to issue
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
    fn make_csr(&self, private_key: &PKey<Private>) -> Result<Option<X509Req>, Box<dyn Error>> {
        let name = self.make_x509_name()?;

        if name.entries().count() == 0 {
            return Ok(None);
        }

        let mut builder = openssl::x509::X509ReqBuilder::new()?;

        // PKCS#10 currently defines version zero.
        builder.set_version(0)?;
        builder.set_subject_name(&name)?;
        builder.set_pubkey(private_key)?;

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
        builder.sign(private_key, signing_digest(private_key))?;

        Ok(Some(builder.build()))
    }

    fn make_x509_name(&self) -> Result<openssl::x509::X509Name, Box<dyn Error>> {
        let mut builder = openssl::x509::X509NameBuilder::new()?;

        if let Some(cn) = &self.common_name {
            builder.append_entry_by_text("CN", cn)?;
        }

        if let Some(o) = &self.organization {
            builder.append_entry_by_text("O", o)?;
        }

        if let Some(ou) = &self.organizational_unit {
            builder.append_entry_by_text("OU", ou)?;
        }

        if let Some(c) = &self.country {
            builder.append_entry_by_text("C", c)?;
        }

        if let Some(st) = &self.state {
            builder.append_entry_by_text("ST", st)?;
        }

        if let Some(l) = &self.locality {
            builder.append_entry_by_text("L", l)?;
        }

        Ok(builder.build())
    }
}

async fn create_csr<Ui: crate::Ui>(
    db: &crate::pkiboo::OpenedDb,
    args: &Args,
    task: Ui::TaskHandle,
    public_key: Option<&PKey<Public>>,
) -> Result<(X509Req, Option<PKey<Private>>), Box<dyn Error>> {
    let has_cli_subject = args.make_x509_name()?.entries().next().is_some();

    match (has_cli_subject, &args.csr) {
        (false, Some(csr_file)) => {
            let pem = std::fs::read(csr_file)?;
            Ok((X509Req::from_pem(&pem)?, None))
        }
        (true, Some(_)) => Err("Either --csr or certificate details must be given".into()),
        (false, None) => {
            todo!("We can't yet prompt for CSR details")
        }
        (true, None) => {
            // A locally constructed PKCS#10 request is signed by its subject
            // key as part of construction. It must never exist as an unsigned
            // intermediate request.
            let key_name = args
                .key
                .as_ref()
                .ok_or("--key is required when constructing a CSR")?;
            let private_key = task.load_private_key(db, key_name).await?;

            // The public lookup happened before media was requested. Confirm
            // that the loaded private material is the same managed key.
            if let Some(public_key) = public_key
                && !private_key.public_eq(public_key)
            {
                return Err(format!("Private key {key_name} does not match its public key").into());
            }

            let csr = args
                .make_csr(&private_key)?
                .ok_or::<String>("Certificate subject is empty".into())?;

            // A root uses this same key to sign its certificate, so retain it
            // for the immediately following issuance task. An intermediate is
            // signed by its issuer; drop the subject key as soon as its CSR is
            // complete instead of retaining unrelated private material.
            let reusable_signing_key = if args.by.is_none() {
                Some(private_key)
            } else {
                None
            };

            Ok((csr, reusable_signing_key))
        }
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

    // RFC 5280 limits serial numbers to 20 octets. Keeping the high bit clear
    // produces a positive integer while retaining 159 random bits.
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

    // X.509 encodes version three as the integer value two.
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

    // A CSR only requests extensions. Certificate policy decides what is
    // actually granted, so requested extensions are not copied verbatim.
    builder.append_extension(BasicConstraints::new().critical().ca().build()?)?;
    builder.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()?,
    )?;
    let subject_key_identifier = SubjectKeyIdentifier::new()
        .build(&builder.x509v3_context(issuer.map(|certificate| certificate.as_ref()), None))?;
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

    let (csr, reusable_signing_key) = boo
        .ui()
        .task("Create CSR".into(), async |task| {
            create_csr::<Ui>(&db, args, task, public_key.as_ref()).await
        })
        .await?;

    // The request must identify a key managed by Pkiboo. If --key was given,
    // also ensure it names the same key carried by the CSR.
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

    check_csr_policy(&csr)?;

    // With no issuer, the subject key signs its own root certificate.
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

    let certificate = boo
        .ui()
        .task("Issue certificate".into(), async |issuance| {
            let signing_key = match &reusable_signing_key {
                Some(signing_key) => signing_key.clone(),
                None => {
                    issuance
                        .task("Load signing key".into(), async |loading| {
                            loading.load_private_key(&db, &signing_key_name).await
                        })
                        .await?
                }
            };

            // Refuse to issue if database metadata points at the wrong key.
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

            // Verify before the public certificate is committed to the DB.
            if !certificate.verify(&signing_key)? {
                return Err("Issued certificate signature could not be verified".into());
            }

            Ok(certificate)
        })
        .await?;

    let name = certificate_name(&csr)?;

    if db.lookup_cert(&name).is_some() {
        return Err(format!("Certificate {name} already exists").into());
    }

    // The complete public PEM is safe to retain in the ordinary database.
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
