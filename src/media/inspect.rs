use std::error::Error;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    /// Path or backend identifier to inspect
    target: Option<PathBuf>,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _media: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
