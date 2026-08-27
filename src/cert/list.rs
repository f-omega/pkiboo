use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Only show certificates issued by this certificate
    #[arg(long)]
    by: Option<String>,

    #[command(flatten)]
    list_options: crate::util::ListOptions,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _cert: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
