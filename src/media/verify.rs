use super::assessment::{MediaAssessment, MediaIssue, VerifiedMediaFile};
use crate::ui::{
    ListItem, ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView,
    TaskStarterExt,
};
use futures::future::try_join_all;
use std::error::Error;
use std::path::PathBuf;

struct IssueRow {
    issue: String,
    path: String,
    detail: String,
}

impl From<&MediaIssue> for IssueRow {
    fn from(issue: &MediaIssue) -> Self {
        match issue {
            MediaIssue::MissingManifest => Self {
                issue: "missing manifest".into(),
                path: String::new(),
                detail: "The medium has no manifest".into(),
            },
            MediaIssue::UnreadableManifest { message } => Self {
                issue: "unreadable manifest".into(),
                path: String::new(),
                detail: message.clone(),
            },
            MediaIssue::UnsafePath { path } => {
                Self::with_path("unsafe path", path, "Path escapes the media root")
            }
            MediaIssue::DuplicateManifestEntry { path } => Self::with_path(
                "duplicate entry",
                path,
                "Path occurs more than once in the manifest",
            ),
            MediaIssue::InvalidManifestEntry { path, message } => {
                Self::with_path("invalid entry", path, message)
            }
            MediaIssue::MissingExpectedEntry { path, kind, name } => Self::with_path(
                "missing expected entry",
                path,
                &format!("Expected {kind} {name}"),
            ),
            MediaIssue::UnexpectedManifestEntry { path } => Self::with_path(
                "unexpected entry",
                path,
                "Entry is not expected by the database",
            ),
        }
    }
}

impl IssueRow {
    fn with_path(issue: &str, path: &PathBuf, detail: &str) -> Self {
        Self {
            issue: issue.into(),
            path: path.display().to_string(),
            detail: detail.into(),
        }
    }
}

impl ListItem for IssueRow {
    fn column_names() -> &'static [&'static str] {
        &["issue", "path", "detail"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.issue.clone(),
            1 => self.path.clone(),
            2 => self.detail.clone(),
            _ => String::new(),
        }
    }
}

impl ListItem for VerifiedMediaFile {
    fn column_names() -> &'static [&'static str] {
        &["path", "signing key"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.path.display().to_string(),
            1 => self.signing_key.clone(),
            _ => String::new(),
        }
    }
}

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    media: super::MediaRef,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _media: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let media_id = args.media.resolve(&db)?;
    let media = db
        .lookup_media_by_id(&media_id)
        .ok_or_else(|| format!("Could not find media {media_id}"))?
        .clone();

    let assessment: MediaAssessment = boo
        .ui()
        .task(format!("Assess media {}", media.label), async |_task| {
            let backend = media.id.open_backend().await?;
            backend.wait_for_available().await?;
            MediaAssessment::collect(&db, &media, backend).await
        })
        .await?;

    let healthy = assessment.is_healthy();
    let issue_count = assessment.issues.len();
    let verified_count = assessment.verified_files.len();
    let media_name = assessment.media.to_string();
    let checked_at = assessment.checked_at.to_rfc3339();
    let issues = assessment
        .issues
        .iter()
        .map(IssueRow::from)
        .collect::<Vec<_>>();
    let verified_files = assessment.verified_files;

    let summary = boo.ui().pane(
        "Assessment".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.property_list(PropertyList::new([
                Property::new("Media", media_name),
                Property::new("Checked at", checked_at),
                Property::new("Healthy", if healthy { "yes" } else { "no" }),
                Property::new("Issues", issue_count.to_string()),
                Property::new("Verified files", verified_count.to_string()),
            ]))
            .display()
            .await;
            Ok(())
        },
    );

    let issue_list = boo.ui().pane(
        "Issues".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.list(issues).display().await;
            Ok(())
        },
    );

    let verified_list = boo.ui().pane(
        "Verified files".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.list(verified_files).display().await;
            Ok(())
        },
    );

    // Start every pane before awaiting any of them. The CLI renders them in
    // order, while a graphical UI can display the complete assessment at once.
    try_join_all([summary, issue_list, verified_list]).await?;

    Ok(())
}
