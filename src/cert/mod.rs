use std::error::Error;

mod create;

#[derive(clap::Parser)]
pub struct Args {
    #[clap(subcommand)]
    command: Command
}

#[derive(clap::Subcommand)]
enum Command {
    Create(create::Args)
}

pub async fn main<Ui: crate::Ui>(boo: &crate::pkiboo::PkiBoo<Ui>,
                                 args: &Args) -> Result<(), Box<dyn Error>> {
    match &args.command {
        Command::Create(c) => create::main(boo, args, c).await
    }
}
