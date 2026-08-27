use crate::ui::{PaneStarterExt, Presenter, Property, PropertyList, PropertyListView};
use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    media: super::MediaRef,
    /// Forget media even when some private material has no fresh verified copy elsewhere
    #[arg(long)]
    force: bool,

    /// Allow forgetting media even when routine copy/share redundancy is reduced
    #[arg(long)]
    allow_reduced_redundancy: bool,

    /// Check whether the medium can be safely forgotten without changing anything
    #[arg(long)]
    dry_run: bool,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _media: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let media_id = args.media.resolve(&db)?;
    let media = db
        .lookup_media_by_id(&media_id)
        .ok_or_else(|| format!("Could not find media {media_id}"))?
        .clone();

    let now = chrono::Utc::now();
    let affected_entities = db.find_media_entities(&media.label).collect::<Vec<_>>();
    let verification_blockers = affected_entities
        .iter()
        .copied()
        .filter(|entity| !entity.has_fresh_verification_elsewhere(&media.label, now))
        .map(|entity| format!("{} {}", entity.kind(), entity.name()))
        .collect::<Vec<_>>();
    let redundancy_blockers = affected_entities
        .iter()
        .copied()
        .filter_map(|entity| {
            let remaining = entity
                .backups()
                .iter()
                .filter(|backup| *backup != &media.label)
                .count();
            (remaining < entity.required_replicas()).then(|| {
                format!(
                    "{} {} ({} remaining, {} required)",
                    entity.kind(),
                    entity.name(),
                    remaining,
                    entity.required_replicas()
                )
            })
        })
        .collect::<Vec<_>>();

    if args.dry_run {
        let safe_by_default = verification_blockers.is_empty() && redundancy_blockers.is_empty();
        let allowed_by_options = (verification_blockers.is_empty() || args.force)
            && (redundancy_blockers.is_empty() || args.allow_reduced_redundancy || args.force);
        boo.ui()
            .pane(
                "Forget assessment".into(),
                async |pane| -> Result<(), Box<dyn Error>> {
                    pane.property_list(PropertyList::new([
                        Property::new("Media", media.label.to_string()),
                        Property::new(
                            "Can forget safely",
                            if safe_by_default { "yes" } else { "no" },
                        ),
                        Property::new(
                            "Allowed with supplied options",
                            if allowed_by_options { "yes" } else { "no" },
                        ),
                        Property::new(
                            "Verification blockers",
                            if verification_blockers.is_empty() {
                                "none".into()
                            } else {
                                verification_blockers.join(", ")
                            },
                        ),
                        Property::new(
                            "Redundancy blockers",
                            if redundancy_blockers.is_empty() {
                                "none".into()
                            } else {
                                redundancy_blockers.join(", ")
                            },
                        ),
                    ]))
                    .display()
                    .await;
                    Ok(())
                },
            )
            .await?;
        return Ok(());
    }

    if !verification_blockers.is_empty() && !args.force {
        return Err(format!(
            "Cannot forget media {} because these private entities have no fresh verified copy on another medium: {}. Verify another copy or pass --force",
            media.label,
            verification_blockers.join(", ")
        )
        .into());
    }

    if !redundancy_blockers.is_empty() && !args.allow_reduced_redundancy && !args.force {
        return Err(format!(
            "Cannot forget media {} because redundancy would fall below policy for: {}. Add more copies, pass --allow-reduced-redundancy, or pass --force",
            media.label,
            redundancy_blockers.join(", ")
        )
        .into());
    }

    if !verification_blockers.is_empty() {
        crate::cli_common::warn(format!(
            "Forgetting {} despite missing fresh alternative copies for: {}",
            media.label,
            verification_blockers.join(", ")
        ));
    }

    if !redundancy_blockers.is_empty() {
        crate::cli_common::warn(format!(
            "Forgetting {} with reduced redundancy for: {}",
            media.label,
            redundancy_blockers.join(", ")
        ));
    }

    db.transaction().forget_media(&media.label);
    Ok(())
}
