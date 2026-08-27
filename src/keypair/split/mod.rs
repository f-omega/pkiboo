use std::error::Error;

mod create;
mod list;
mod meta;
mod retire;
mod show;
mod verify;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
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
    /// Retire a recovery split
    Retire(retire::Args),
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _key: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    match &args.command {
        Command::Create(command) => create::main(boo, args, command).await,
        Command::List(command) => list::main(boo, args, command).await,
        Command::Show(command) => show::main(boo, args, command).await,
        Command::Verify(command) => verify::main(boo, args, command).await,
        Command::Meta(command) => meta::main(boo, args, command).await,
        Command::Retire(command) => retire::main(boo, args, command).await,
    }
}
