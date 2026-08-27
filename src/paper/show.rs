use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the paper artifact
    #[arg(long)]
    paper: String,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
