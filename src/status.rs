use crate::pkiboo::{HealthIssue, VERIFICATION_MAX_AGE_DAYS};
use crate::ui::{
    ListItem, ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView,
};
use futures::future::{join_all, try_join_all};
use std::error::Error;

impl ListItem for HealthIssue {
    fn column_names() -> &'static [&'static str] {
        &["severity", "entity", "detail"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.severity.to_string(),
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
    let mut health = db.health_at(chrono::Utc::now())?;

    // Attachment is live backend state rather than database health. Probe all
    // registered media concurrently without waiting for or mounting anything.
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
            Err(error) => health.issues.push(crate::pkiboo::HealthIssue {
                severity: crate::pkiboo::HealthSeverity::Warning,
                entity: format!("media {}", media.label),
                detail: format!("could not check attachment state: {error}"),
            }),
        }
    }

    let warning_count = health.issues.len();
    let healthy_keys = health.healthy_keys;
    let healthy_splits = health.healthy_splits;
    let healthy_active_certificates = health.healthy_active_certificates;
    let active_certificates = health.active_certificates;
    let issues = health.issues;
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
                Property::new(
                    "Healthy keys",
                    format!("{healthy_keys}/{}", db.keys.len()),
                ),
                Property::new(
                    "Healthy splits",
                    format!("{healthy_splits}/{}", db.splits.len()),
                ),
                Property::new(
                    "Healthy active certificates",
                    format!(
                        "{}/{}",
                        healthy_active_certificates, active_certificates
                    ),
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
            pane.list(issues).display().await;
            Ok(())
        },
    );

    // Both panes are available together to graphical backends. The CLI
    // backend buffers them and prints them in creation order.
    try_join_all([summary, warning_list]).await?;

    Ok(())
}
