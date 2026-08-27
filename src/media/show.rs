use futures::future::try_join_all;
use std::error::Error;
use crate::ui::{UiExt, Task};

#[derive(clap::Parser)]
pub struct Args {
    #[command(flatten)]
    media: super::MediaRef
}


pub async fn main<Ui: crate::Ui>(boo: &crate::pkiboo::PkiBoo<Ui>,
                                 _media: &super::Args,
                                 args: &Args) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let media_id = args.media.resolve(&db)?;
    let media = db.lookup_media_by_id(&media_id).ok_or(format!("Could not find media {media_id}"))?;

    try_join_all(vec![
        boo.ui().task("Retrieving data".into(),
                      async |task| {
                          task.property_list(std::vec![
                              ("Name".into(), media.label.to_string().clone()),
                              ("ID".into(), format!("{}", media.id).into()),
                              ("Trusted".into(), if media.trusted { "yes".into() } else { "no".into() }),
                          ]);
                          Ok(())
                      })
    ]).await;
    Ok(())
}
