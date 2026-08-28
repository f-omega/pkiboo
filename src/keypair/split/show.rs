use crate::pkiboo::{PrivateEntity, PrivateEntityVerification, Split};
use crate::ui::{ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView};
use crate::util::Name;
use futures::future::try_join_all;
use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the recovery split
    #[arg(long)]
    split: Name<Split>,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _split: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let split = db
        .lookup_split(&args.split)
        .ok_or_else(|| format!("Split {} not found", args.split))?;
    let now = chrono::Utc::now();
    let distinct_shares = db.distinct_shares_for_split(split).len();
    let missing_shares = db
        .missing_shares_for_split(split)
        .iter()
        .map(|share| share.0.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let verification = match split.verification_status_at(now) {
        PrivateEntityVerification::Complete => "current".into(),
        PrivateEntityVerification::Degraded { verified, expected } => {
            format!("degraded ({verified}/{expected} shares)")
        }
        PrivateEntityVerification::NotVerified => "needed".into(),
    };

    let details = boo.ui().pane(
        "Split details".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.property_list(PropertyList::new([
                Property::new("Name", split.label.to_string()),
                Property::new("Key", split.key.to_string()),
                Property::new("Threshold", split.min_splits.to_string()),
                Property::new("Total shares", split.num_splits.to_string()),
                Property::new("Distinct shares", distinct_shares.to_string()),
                Property::new(
                    "Missing shares",
                    if missing_shares.is_empty() {
                        "none".into()
                    } else {
                        missing_shares
                    },
                ),
                Property::new(
                    "Recovery quorum available",
                    if distinct_shares >= split.min_splits as usize {
                        "yes"
                    } else {
                        "no"
                    },
                ),
                Property::new("Verification", verification),
            ]))
            .display()
            .await;
            Ok(())
        },
    );

    let placements = split.share_placement_statuses_at(now);
    let shares = boo.ui().pane(
        "Share placements".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.list(placements).display().await;
            Ok(())
        },
    );
    let metadata = boo.ui().pane(
        "Metadata".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.property_list(split.meta.properties()).display().await;
            Ok(())
        },
    );

    try_join_all([details, shares, metadata]).await?;
    Ok(())
}
