use std::error::Error;

#[derive(clap::Parser)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Generate a new private key directly onto selected media
    Create(super::create::Args),

    /// List managed keys and their recoverability state
    List(super::list::Args),

    /// Show a managed key
    Show(super::show::Args),
    /// Create a new complete copy of a key
    Backup(super::backups::Args),
    /// Manage threshold recovery splits
    Split(super::split::Args),
    /// Verify a key copy or recovery path
    Verify(super::verify::Args),
    /// Manage metadata on a key
    Meta(super::meta::Args),
    /// Restore a complete key from recovery shares
    Restore(super::split::reconstruct::Args),
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    match &args.command {
        Command::Create(command) => super::create::main(boo, args, command).await,
        Command::List(command) => super::list::main(boo, args, command).await,
        Command::Show(command) => super::show::main(boo, args, command).await,
        Command::Backup(command) => super::backups::main(boo, args, command).await,
        Command::Split(command) => super::split::main(boo, args, command).await,
        Command::Verify(command) => super::verify::main(boo, args, command).await,
        Command::Meta(command) => super::meta::main(boo, args, command).await,
        Command::Restore(command) => super::split::reconstruct::main(boo, command).await,
    }
}
