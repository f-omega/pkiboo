use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Only show splits belonging to this key
    #[arg(long)]
    key: Option<String>,
    #[command(flatten)]
    list_options: crate::util::ListOptions,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _split: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
