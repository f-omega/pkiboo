use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Scanner device to use
    #[arg(long)]
    device: Option<String>,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
