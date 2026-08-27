use crate::ui::{
    ListItem, ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView,
};
use futures::{FutureExt, future::try_join_all};
use std::error::Error;

/// One entity that prevents a medium from passing a forget safety policy.
/// Keeping this as a list model lets every UI choose an appropriate full-list
/// presentation instead of squeezing blockers into a summary string.
struct ForgetBlocker {
    entity: String,
}

impl ListItem for ForgetBlocker {
    fn column_names() -> &'static [&'static str] {
        &["entity"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.entity.clone(),
            _ => String::new(),
        }
    }
}

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
    // Resolve the CLI selector before doing any policy work. The friendly
    // label is the stable reference used by private-entity backup records.
    let mut db = boo.open_database()?;
    let media_id = args.media.resolve(&db)?;
    let media = db
        .lookup_media_by_id(&media_id)
        .ok_or_else(|| format!("Could not find media {media_id}"))?
        .clone();

    // Take one timestamp for the whole assessment so records near the
    // freshness boundary cannot receive inconsistent classifications.
    let now = chrono::Utc::now();

    // Only entities assigned to this medium can be made less recoverable by
    // forgetting it. Keep the trait objects so keys and numbered split shares
    // apply their own verification and redundancy rules.
    let affected_entities = db.find_media_entities(&media.label).collect::<Vec<_>>();

    // Verification safety and redundancy are deliberately independent. A
    // copy elsewhere may satisfy the count policy while still being too stale
    // to trust, and a freshly verified copy can still leave too few replicas.
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
            let redundancy = entity.redundancy_after_removing(&media.label);
            (!redundancy.is_sufficient()).then(|| {
                format!(
                    "{} {} ({} remaining, {} required)",
                    entity.kind(),
                    entity.name(),
                    redundancy.remaining,
                    redundancy.required
                )
            })
        })
        .collect::<Vec<_>>();

    // A dry run reports both the default safety decision and the decision
    // after applying the supplied overrides. It never reaches the database
    // mutation below, even when --force is also present.
    if args.dry_run {
        let safe_by_default = verification_blockers.is_empty() && redundancy_blockers.is_empty();
        let allowed_by_options = (verification_blockers.is_empty() || args.force)
            && (redundancy_blockers.is_empty() || args.allow_reduced_redundancy || args.force);
        // Drive every pane concurrently. The CLI buffers them in creation
        // order, while a graphical UI can populate and display all panes at
        // once instead of waiting for the summary to finish first.
        let mut panes = vec![
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
                                verification_blockers.len().to_string(),
                            ),
                            Property::new(
                                "Redundancy blockers",
                                redundancy_blockers.len().to_string(),
                            ),
                        ]))
                        .display()
                        .await;
                        Ok(())
                    },
                )
                .boxed_local(),
        ];

        if !verification_blockers.is_empty() {
            panes.push(
                boo.ui()
                    .pane(
                        "Verification blockers".into(),
                        async |pane| -> Result<(), Box<dyn Error>> {
                            pane.list(
                                verification_blockers
                                    .iter()
                                    .cloned()
                                    .map(|entity| ForgetBlocker { entity })
                                    .collect::<Vec<_>>(),
                            )
                            .display()
                            .await;
                            Ok(())
                        },
                    )
                    .boxed_local(),
            );
        }

        if !redundancy_blockers.is_empty() {
            panes.push(
                boo.ui()
                    .pane(
                        "Redundancy blockers".into(),
                        async |pane| -> Result<(), Box<dyn Error>> {
                            pane.list(
                                redundancy_blockers
                                    .iter()
                                    .cloned()
                                    .map(|entity| ForgetBlocker { entity })
                                    .collect::<Vec<_>>(),
                            )
                            .display()
                            .await;
                            Ok(())
                        },
                    )
                    .boxed_local(),
            );
        }

        try_join_all(panes).await?;

        return Ok(());
    }

    // Missing fresh evidence is the strongest blocker. Only --force may
    // override it; accepting reduced redundancy does not imply accepting an
    // unverified remaining copy.
    if !verification_blockers.is_empty() && !args.force {
        return Err(format!(
            "Cannot forget media {} because these private entities have no fresh verified copy on another medium: {}. Verify another copy or pass --force",
            media.label,
            verification_blockers.join(", ")
        )
        .into());
    }

    // Reduced redundancy has its own narrower override. --force remains a
    // superset for explicit emergency or loss-handling workflows.
    if !redundancy_blockers.is_empty() && !args.allow_reduced_redundancy && !args.force {
        return Err(format!(
            "Cannot forget media {} because redundancy would fall below policy for: {}. Add more copies, pass --allow-reduced-redundancy, or pass --force",
            media.label,
            redundancy_blockers.join(", ")
        )
        .into());
    }

    // Overrides should remain conspicuous even though the operator explicitly
    // requested them; otherwise a successful command could hide degradation.
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

    // Perform one transactional mutation only after every policy gate passes.
    // forget_media removes inventory, backup placements, and associated
    // verification evidence together so derived status changes atomically.
    db.transaction().forget_media(&media.label);
    Ok(())
}
