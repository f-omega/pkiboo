use std::error::Error;

mod forget;
mod import;
mod list;
mod meta;
mod scan;
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
    /// Scan a printed pkiboo artifact
    Scan(scan::Args),
    /// Import a PDF or scanned file
    Import(import::Args),
    /// Verify that a paper artifact is readable and valid
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
        Command::Scan(command) => scan::main(boo, args, command).await,
        Command::Import(command) => import::main(boo, args, command).await,
        Command::Verify(command) => verify::main(boo, args, command).await,
        Command::Meta(command) => meta::main(boo, args, command).await,
        Command::Forget(command) => forget::main(boo, args, command).await,
    }
}
