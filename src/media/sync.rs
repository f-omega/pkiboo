use crate::ui::TaskStarterExt;
use std::error::Error;

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

    boo.ui()
        .task(format!("Sync database hint to {}", media.label), async |_task| {
            let backend = media.id.open_backend().await?;
            let sync = async {
                backend.wait_for_available().await?;
                db.write_recovery_hint(backend.clone()).await
            }
            .await;
            let release = backend.release().await;

            match (sync, release) {
                (Ok(()), Ok(_)) => Ok(()),
                (Err(error), _) => Err(error),
                (Ok(()), Err(error)) => Err(error),
            }
        })
        .await
}
