use crate::pkiboo::{Key, PrivateEntity, PrivateEntityVerification};
use crate::ui::ListView;
use crate::util::Name;
use std::error::Error;

struct ListedSplit {
    name: String,
    key: String,
    threshold: String,
    placements: usize,
    verification: String,
}

impl crate::ui::ListItem for ListedSplit {
    fn column_names() -> &'static [&'static str] {
        &["name", "key", "threshold", "placements", "verification"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.name.clone(),
            1 => self.key.clone(),
            2 => self.threshold.clone(),
            3 => self.placements.to_string(),
            4 => self.verification.clone(),
            _ => String::new(),
        }
    }
}

#[derive(clap::Args)]
pub struct Args {
    /// Only show splits belonging to this key
    #[arg(long)]
    key: Option<Name<Key>>,

    #[command(flatten)]
    list_options: crate::util::ListOptions,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _split: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let now = chrono::Utc::now();
    let splits = match &args.key {
        Some(key) => db.splits_for_key(key).collect::<Vec<_>>(),
        None => db.splits.iter().collect::<Vec<_>>(),
    }
    .into_iter()
    .map(|split| ListedSplit {
        name: split.label.to_string(),
        key: split.key.to_string(),
        threshold: format!("{} of {}", split.min_splits, split.num_splits),
        placements: split.backups.len(),
        verification: match split.verification_status_at(now) {
            PrivateEntityVerification::Complete => "current".into(),
            PrivateEntityVerification::Degraded { verified, expected } => {
                format!("degraded ({verified}/{expected})")
            }
            PrivateEntityVerification::NotVerified => "needed".into(),
        },
    })
    .collect::<Vec<_>>();

    boo.ui()
        .list(splits)
        .with_options(&args.list_options)
        .display()
        .await;
    Ok(())
}
