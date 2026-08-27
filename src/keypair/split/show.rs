use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the recovery split
    split: String,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _split: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
