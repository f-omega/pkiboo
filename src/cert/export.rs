use std::error::Error;
use std::path::PathBuf;

#[derive(Clone, clap::ValueEnum)]
enum Format {
    Pem,
    Der,
}

#[derive(clap::Args)]
pub struct Args {
    /// Name of the certificate
    #[arg(long)]
    cert: String,

    /// Certificate encoding
    #[arg(long, value_enum, default_value = "pem")]
    format: Format,

    /// Write the certificate to this file instead of stdout
    #[arg(long)]
    output: Option<PathBuf>,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _cert: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
