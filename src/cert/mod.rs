use std::error::Error;

mod create;
mod list;
mod meta;
mod retire;
mod show;

#[derive(clap::Parser)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Create a certificate
    Create(create::Args),
    /// List managed certificates
    List(list::Args),
    /// Show a certificate
    Show(show::Args),
    /// Manage metadata on a certificate
    Meta(meta::Args),
    /// Retire a certificate from new issuance
    Retire(retire::Args),
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    match &args.command {
        Command::Create(command) => create::main(boo, args, command).await,
        Command::List(command) => list::main(boo, args, command).await,
        Command::Show(command) => show::main(boo, args, command).await,
        Command::Meta(command) => meta::main(boo, args, command).await,
        Command::Retire(command) => retire::main(boo, args, command).await,
    }
}
