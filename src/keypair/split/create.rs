use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Key to split
    #[arg(long)]
    key: String,
    /// Name for the recovery split
    #[arg(long)]
    name: String,
    /// Shares required to reconstruct the key
    #[arg(long)]
    threshold: usize,
    /// Total number of shares to create
    #[arg(long)]
    shares: usize,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _split: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
