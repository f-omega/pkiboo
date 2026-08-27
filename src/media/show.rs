use crate::ui::{ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView};
use futures::future::try_join;
use std::error::Error;

struct ExpectedEntity {
    emoji: String,
    kind: String,
    name: String,
}

impl crate::ui::ListItem for ExpectedEntity {
    fn column_names() -> &'static [&'static str] {
        &["", "kind", "name"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.emoji.clone(),
            1 => self.kind.clone(),
            2 => self.name.clone(),
            _ => String::new(),
        }
    }
}

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

    // This is desired state from the database. It deliberately does not imply
    // that an attached medium currently contains readable, verified copies.
    let expected_entities = db
        .find_media_entities(&media.label)
        .map(|entity| ExpectedEntity {
            emoji: entity.emoji().to_owned(),
            kind: entity.kind().to_owned(),
            name: entity.name().clone(),
        })
        .collect::<Vec<_>>();

    let expected_contents = boo.ui().pane(
        "Expected contents".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.list(expected_entities).display().await;
            Ok(())
        },
    );

    if args.contents {
        expected_contents.await?;
    } else {
        let media_details = boo.ui().pane(
            "Media details".into(),
            async |pane| -> Result<(), Box<dyn Error>> {
                pane.property_list(PropertyList::new([
                    Property::new("Name", media.label.to_string()),
                    Property::new("ID", media.id.to_string()),
                    Property::new("Trusted", if media.trusted { "yes" } else { "no" }),
                ]))
                .display()
                .await;

                Ok(())
            },
        );

        // Both panes are driven concurrently. The CLI backend buffers their
        // output and flushes it in pane creation order; a graphical backend
        // can expose and populate both panes independently.
        try_join(media_details, expected_contents).await?;
    }

    Ok(())
}
