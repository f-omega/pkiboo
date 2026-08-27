use crate::ui::{Presenter, Property, PropertyList, PropertyListView, TaskStarterExt};
use futures::future::try_join_all;
use std::error::Error;

#[derive(clap::Parser)]
pub struct Args {
    #[command(flatten)]
    media: super::MediaRef,

    /// Display only the contents stored on the medium
    #[arg(long)]
    contents: bool,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::pkiboo::PkiBoo<Ui>,
    _media: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let media_id = args.media.resolve(&db)?;
    let media = db
        .lookup_media_by_id(&media_id)
        .ok_or(format!("Could not find media {media_id}"))?;

    try_join_all(vec![boo.ui().task(
        "Retrieving data".into(),
        async |task| {
            task.property_list(PropertyList::titled(
                "Media",
                [
                    Property::new("Name", media.label.to_string()),
                    Property::new("ID", media.id.to_string()),
                    Property::new("Trusted", if media.trusted { "yes" } else { "no" }),
                ],
            ))
            .display()
            .await;
            Ok(())
        },
    )])
    .await?;
    Ok(())
}
