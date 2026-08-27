use std::error::Error;

#[derive(clap::Parser)]
pub struct Args {
    #[command(flatten)]
    media_ref: super::MediaRef,

    #[command(flatten)]
    meta: crate::pkiboo::MetaSetArgs,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::pkiboo::PkiBoo<Ui>,
    _media: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let media_id = args.media_ref.resolve(&db)?;

    let mut new_media = match db.lookup_media_by_id(&media_id) {
        None => panic!("Media {media_id} not found"),
        Some(m) => m.clone(),
    };

    let mut tx = db.transaction();
    new_media.meta.manage(boo.ui(), &args.meta).await;
    tx.update_media(new_media)?;
    Ok(())
}

#[derive(clap::Parser)]
pub struct Rename {
    #[command(flatten)]
    media_ref: super::MediaRef,

    /// New name to assign
    #[arg(long)]
    label: String,
}

pub async fn rename<Ui: crate::Ui>(
    boo: &crate::pkiboo::PkiBoo<Ui>,
    _media: &super::Args,
    args: &Rename,
) -> Result<(), Box<dyn Error>> {
    let mut db = boo.open_database()?;
    let media_id = args.media_ref.resolve(&db)?;

    let mut new_media = match db.lookup_media_by_id(&media_id) {
        None => panic!("Media {media_id} not found"),
        Some(m) => m.clone(),
    };

    let mut tx = db.transaction();
    new_media.label = crate::util::Name::new(args.label.clone());
    tx.update_media(new_media)?;
    Ok(())
}
