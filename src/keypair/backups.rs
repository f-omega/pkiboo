use std::error::Error;

#[derive(clap::Parser)]
pub struct Args {
    /// Key to copy
    key: String,

    /// Destination media for the new complete copy
    #[arg(long, required = true)]
    media: Vec<String>,
}

pub(crate) async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _keypair: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
