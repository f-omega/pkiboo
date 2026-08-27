use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the certificate
    cert: String,

    /// Reason for retiring the certificate
    #[arg(long)]
    reason: Option<String>,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _cert: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
