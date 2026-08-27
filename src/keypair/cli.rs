use std::error::Error;

#[derive(clap::Parser)]
pub struct Args {
    #[command(subcommand)]
    command: Command
}

#[derive(clap::Subcommand)]
enum Command {
    Create(super::create::Args),
    List(super::list::Args),

    /// Create a new backup for this key
    Backup(super::backups::Args),
}


pub async fn main<Ui: crate::Ui>(boo: &crate::pkiboo::PkiBoo<Ui>, args: &Args) -> Result<(), Box<dyn Error>> {
    match &args.command {
        Command::Create(c) => super::create::main(boo, args, c).await,
        Command::List(c) => super::list::main(boo, args, c).await,
        Command::Backup(c) => super::backups::main(boo, args, c).await
    }
}

