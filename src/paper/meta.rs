use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the paper artifact
    paper: String,

    #[command(flatten)]
    meta: crate::pkiboo::MetaSetArgs,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    // Paper artifacts do not have a persisted entity yet.
    todo!()
}
