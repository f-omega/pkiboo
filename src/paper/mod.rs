use std::error::Error;

pub(crate) mod assembler;
mod forget;
pub(crate) mod input;
mod list;
mod meta;
pub(crate) mod pdf;
mod show;
mod verify;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// List registered paper artifacts
    List(list::Args),
    /// Show a paper artifact
    Show(show::Args),
    /// Verify that one paper share is cryptographically valid
    Verify(verify::Args),
    /// Manage metadata on a paper artifact
    Meta(meta::Args),
    /// Forget a lost or destroyed paper artifact
    Forget(forget::Args),
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    match &args.command {
        Command::List(command) => list::main(boo, args, command).await,
        Command::Show(command) => show::main(boo, args, command).await,
        Command::Verify(command) => verify::main(boo, args, command).await,
        Command::Meta(command) => meta::main(boo, args, command).await,
        Command::Forget(command) => forget::main(boo, args, command).await,
    }
}
