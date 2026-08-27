use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    media: super::MediaRef,
    /// Reason for retiring the medium
    #[arg(long)]
    reason: Option<String>,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _media: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
