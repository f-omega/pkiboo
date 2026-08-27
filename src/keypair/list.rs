use std::error::Error;
use crate::ui::ListView;

#[derive(clap::Parser)]
pub struct Args {
    #[command(flatten)]
    list_options: crate::util::ListOptions
}

pub(crate) async fn main<Ui: crate::Ui>
    (boo: &crate::PkiBoo<Ui>,
     keypair: &super::Args,
     args: &Args) -> Result<(), Box<dyn Error>>
{
    let mut db = boo.open_database()?;
    boo.ui().list(db.keys.clone()).with_options(&args.list_options).display().await;
    Ok(())
}
