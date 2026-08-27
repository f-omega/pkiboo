use crate::pkiboo::{Key, Media};
use crate::util::Name;
use std::error::Error;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    /// Managed public key that the reconstructed private key must match
    #[arg(long)]
    key: Name<Key>,

    /// Share file in any supported historical format; repeat as needed
    #[arg(long)]
    input: Vec<PathBuf>,

    /// Media onto which the reconstructed complete key should be written
    #[arg(long)]
    media: Vec<Name<Media>>,
}

pub async fn main<Ui: crate::Ui>(
    _boo: &crate::PkiBoo<Ui>,
    _split: &super::Args,
    _args: &Args,
) -> Result<(), Box<dyn Error>> {
    // Historical decoders will normalize input into numbered shares. Accept
    // reconstruction only when the private key's public component equals the
    // managed public key, then write it directly to the requested media.
    todo!("decode shares, reconstruct and validate the key, then write it to media")
}
