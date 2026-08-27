use std::error::Error;

use crate::ui::ListView;

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

    // TODO: only_online

    boo.ui()
        .list(db.media.clone())
        .with_options(&args.list_options)
        .display()
        .await;
    Ok(())
}
