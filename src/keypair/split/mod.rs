use std::error::Error;

mod backup;
mod create;
mod list;
mod meta;
pub(crate) mod reconstruct;
pub(crate) mod share;
mod show;
mod verify;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Copy a numbered recovery share to alternate media
    Backup(backup::Args),
    /// Create a threshold recovery split
    Create(create::Args),
    /// List recovery splits
    List(list::Args),
    /// Show a recovery split
    Show(show::Args),
    /// Verify a recovery split
    Verify(verify::Args),
    /// Manage metadata on a recovery split
    Meta(meta::Args),
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _key: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    match &args.command {
        Command::Backup(command) => backup::main(boo, args, command).await,
        Command::Create(command) => create::main(boo, args, command).await,
        Command::List(command) => list::main(boo, args, command).await,
        Command::Show(command) => show::main(boo, args, command).await,
        Command::Verify(command) => verify::main(boo, args, command).await,
        Command::Meta(command) => meta::main(boo, args, command).await,
    }
}
