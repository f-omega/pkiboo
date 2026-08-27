use crate::ui::{ListView, Presenter, Property, PropertyList, PropertyListView, TaskStarterExt};
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

    if !args.contents {
        boo.ui()
            .task("Media details".into(), async |task| {
                task.property_list(PropertyList::new([
                    Property::new("Name", media.label.to_string()),
                    Property::new("ID", media.id.to_string()),
                    Property::new("Trusted", if media.trusted { "yes" } else { "no" }),
                ]))
                .display()
                .await;

                Ok(())
            })
            .await?;
    }

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

    boo.ui()
        .task("Expected contents".into(), async |task| {
            task.list(expected_entities).display().await;
            Ok(())
        })
        .await?;

    Ok(())
}
