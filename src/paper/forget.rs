use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the paper artifact
    #[arg(long)]
    paper: String,
    /// Confirm that the artifact should no longer count toward recovery
    #[arg(long)]
    confirm: bool,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
