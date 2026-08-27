use crate::pkiboo::{
    MIN_KEY_REPLICAS, PrivateEntity, PrivateEntityVerification, VERIFICATION_MAX_AGE_DAYS,
};
use crate::ui::{
    ListItem, ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView,
};
use futures::future::{join_all, try_join_all};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::error::Error;

const CERT_EXPIRY_WARNING_DAYS: u32 = 30;

struct StatusWarning {
    severity: &'static str,
    entity: String,
    detail: String,
}

impl StatusWarning {
    fn warning(entity: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            severity: "warning",
            entity: entity.into(),
            detail: detail.into(),
        }
    }

    fn critical(entity: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            severity: "critical",
            entity: entity.into(),
            detail: detail.into(),
        }
    }
}

impl ListItem for StatusWarning {
    fn column_names() -> &'static [&'static str] {
        &["severity", "entity", "detail"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.severity.into(),
            1 => self.entity.clone(),
            2 => self.detail.clone(),
            _ => String::new(),
        }
    }
}

#[derive(clap::Args)]
pub struct Args {}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let now = chrono::Utc::now();
    let mut warnings = Vec::new();

    let mut healthy_keys = 0;
    for key in &db.keys {
        let entity = format!("key {}", key.name);
        let enough_replicas = key.backups.len() >= MIN_KEY_REPLICAS;
        let references_valid = key
            .backups
            .iter()
            .all(|media| db.lookup_media(media).is_some());

        if !enough_replicas {
            let detail = format!(
                "has {} complete copies; policy requires {MIN_KEY_REPLICAS}",
                key.backups.len()
            );
            if key.backups.is_empty() {
                warnings.push(StatusWarning::critical(entity.clone(), detail));
            } else {
                warnings.push(StatusWarning::warning(entity.clone(), detail));
            }
        }

        let verification = key.verification_status_at(now);
        add_verification_warning(&mut warnings, &entity, verification);
        if enough_replicas
            && references_valid
            && verification == PrivateEntityVerification::Complete
        {
            healthy_keys += 1;
        }

        for media in &key.backups {
            if db.lookup_media(media).is_none() {
                warnings.push(StatusWarning::critical(
                    entity.clone(),
                    format!("references unknown media {media}"),
                ));
            }
        }
    }

    let mut healthy_splits = 0;
    for split in &db.splits {
        let entity = format!("split {}", split.label);
        let references_valid = db.lookup_key(&split.key).is_some()
            && split
                .backups
                .iter()
                .all(|backup| db.lookup_media(&backup.media).is_some());
        let available_shares = split
            .backups
            .iter()
            .map(|backup| backup.share)
            .collect::<HashSet<_>>()
            .len();
        let complete = available_shares >= split.num_splits as usize;

        if available_shares < split.min_splits as usize {
            warnings.push(StatusWarning::critical(
                entity.clone(),
                format!(
                    "only {available_shares} distinct shares are recorded; {} are required for recovery",
                    split.min_splits
                ),
            ));
        } else if !complete {
            warnings.push(StatusWarning::warning(
                entity.clone(),
                format!(
                    "only {available_shares} of {} configured shares are recorded",
                    split.num_splits
                ),
            ));
        }

        let verification = split.verification_status_at(now);
        add_verification_warning(&mut warnings, &entity, verification);
        if complete && references_valid && verification == PrivateEntityVerification::Complete {
            healthy_splits += 1;
        }

        if db.lookup_key(&split.key).is_none() {
            warnings.push(StatusWarning::critical(
                entity.clone(),
                format!("references unknown key {}", split.key),
            ));
        }
        for backup in &split.backups {
            if db.lookup_media(&backup.media).is_none() {
                warnings.push(StatusWarning::critical(
                    entity.clone(),
                    format!(
                        "share {} references unknown media {}",
                        backup.share.0, backup.media
                    ),
                ));
            }
        }
    }

    let openssl_now = openssl::asn1::Asn1Time::days_from_now(0)?;
    let expiry_warning = openssl::asn1::Asn1Time::days_from_now(CERT_EXPIRY_WARNING_DAYS)?;
    let mut healthy_active_certs = 0;
    let active_cert_count = db
        .certs
        .iter()
        .filter(|cert| cert.retired_at.is_none())
        .count();

    for cert in &db.certs {
        let entity = format!("certificate {}", cert.name);
        let key_exists = db.lookup_key(&cert.key).is_some();
        let issuer_exists = cert
            .issuer
            .as_ref()
            .is_none_or(|issuer| db.lookup_cert(issuer).is_some());

        if !key_exists {
            warnings.push(StatusWarning::critical(
                entity.clone(),
                format!("references unknown key {}", cert.key),
            ));
        }
        if let Some(issuer) = &cert.issuer
            && !issuer_exists
        {
            warnings.push(StatusWarning::critical(
                entity.clone(),
                format!("references unknown issuer {issuer}"),
            ));
        }

        // Retired certificates are retained deliberately for history and
        // chain construction, so expiry is actionable only for active ones.
        if cert.retired_at.is_some() {
            continue;
        }

        match openssl::x509::X509::from_pem(cert.certificate.as_bytes()) {
            Err(error) => warnings.push(StatusWarning::critical(
                entity,
                format!("stored certificate PEM is invalid: {error}"),
            )),
            Ok(certificate) => {
                if certificate.not_before().compare(&openssl_now)? == Ordering::Greater {
                    warnings.push(StatusWarning::critical(entity, "is not valid yet"));
                } else if certificate.not_after().compare(&openssl_now)? == Ordering::Less {
                    warnings.push(StatusWarning::critical(entity, "has expired"));
                } else if certificate.not_after().compare(&expiry_warning)? != Ordering::Greater {
                    warnings.push(StatusWarning::warning(
                        entity,
                        format!("expires within {CERT_EXPIRY_WARNING_DAYS} days"),
                    ));
                } else if key_exists && issuer_exists {
                    healthy_active_certs += 1;
                }
            }
        }
    }

    // Attachment probes run concurrently and never wait for or mount media.
    // A failed probe belongs in the dashboard rather than aborting all status
    // output and hiding unrelated health information.
    let media_status = join_all(db.media.iter().cloned().map(|media| async move {
        let result = match media.id.open_backend().await {
            Ok(backend) => backend.check_if_available().await,
            Err(error) => Err(error),
        };
        (media, result)
    }))
    .await;
    let mut online_media = 0;
    for (media, result) in media_status {
        match result {
            Ok(true) => online_media += 1,
            Ok(false) => {}
            Err(error) => warnings.push(StatusWarning::warning(
                format!("media {}", media.label),
                format!("could not check attachment state: {error}"),
            )),
        }
    }

    let warning_count = warnings.len();
    let overall = if warning_count == 0 {
        "healthy"
    } else {
        "attention needed"
    };
    let summary = boo.ui().pane(
        "PKI status".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.property_list(PropertyList::new([
                Property::new("Overall", overall),
                Property::new("Warnings", warning_count.to_string()),
                Property::new("Healthy keys", format!("{healthy_keys}/{}", db.keys.len())),
                Property::new(
                    "Healthy splits",
                    format!("{healthy_splits}/{}", db.splits.len()),
                ),
                Property::new(
                    "Healthy active certificates",
                    format!("{healthy_active_certs}/{active_cert_count}"),
                ),
                Property::new("Online media", format!("{online_media}/{}", db.media.len())),
                Property::new(
                    "Verification freshness",
                    format!("{VERIFICATION_MAX_AGE_DAYS} days"),
                ),
            ]))
            .display()
            .await;
            Ok(())
        },
    );

    let warning_list = boo.ui().pane(
        "Warnings".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.list(warnings).display().await;
            Ok(())
        },
    );

    // Both panes are available together to graphical backends. The CLI
    // backend buffers them and prints them in creation order.
    try_join_all([summary, warning_list]).await?;

    Ok(())
}

fn add_verification_warning(
    warnings: &mut Vec<StatusWarning>,
    entity: &str,
    status: PrivateEntityVerification,
) {
    match status {
        PrivateEntityVerification::Complete => {}
        PrivateEntityVerification::Degraded { verified, expected } => {
            warnings.push(StatusWarning::warning(
                entity,
                format!("only {verified} of {expected} copies or shares have fresh verification"),
            ));
        }
        PrivateEntityVerification::NotVerified => warnings.push(StatusWarning::critical(
            entity,
            "has no usable complete verification within the freshness interval",
        )),
    }
}
