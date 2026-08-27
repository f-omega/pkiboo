use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the key
    key: String,

    /// Verify this particular medium
    #[arg(long)]
    media: Option<String>,

    /// Verify this particular recovery split
    #[arg(long)]
    split: Option<String>,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _key: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    todo!()
}
