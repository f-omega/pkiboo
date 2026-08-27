use std::error::Error;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    /// PDF or image containing a paper artifact
    #[arg(long)]
    file: PathBuf,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
