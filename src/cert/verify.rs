use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the certificate
    #[arg(long)]
    cert: String,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _cert: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
