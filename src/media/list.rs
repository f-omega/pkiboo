use std::error::Error;

use crate::ui::ListView;
use futures::future::try_join_all;

struct ListedMedia {
    media: crate::pkiboo::Media,
    online: bool,
}

impl crate::ui::ListItem for ListedMedia {
    fn column_names() -> &'static [&'static str] {
        &["label", "id", "trusted", "online"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.media.label.to_string(),
            1 => self.media.id.to_string(),
            2 => self.media.trusted.to_string(),
            3 => if self.online { "yes" } else { "no" }.into(),
            _ => String::new(),
        }
    }
}

#[derive(clap::Parser)]
pub struct Args {
    /// Only show online devices
    #[arg(long)]
    only_online: bool,

    #[command(flatten)]
    list_options: crate::util::ListOptions,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::pkiboo::PkiBoo<Ui>,
    _media: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;

    // Probe every backend concurrently so the ordinary listing can report
    // attachment state too. This is a udev snapshot: it never waits or mounts.
    let mut media = try_join_all(db.media.iter().cloned().map(async |media| {
        let backend = media.id.open_backend().await?;
        Ok::<_, Box<dyn Error>>(ListedMedia {
            media,
            online: backend.check_if_available().await?,
        })
    }))
    .await?;

    if args.only_online {
        media.retain(|media| media.online);
    }

    boo.ui()
        .list(media)
        .with_options(&args.list_options)
        .display()
        .await;
    Ok(())
}
