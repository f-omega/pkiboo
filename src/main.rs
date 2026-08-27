use clap::{Parser,Subcommand};

mod media;
mod cli_common;
mod multihash;
mod util;
mod pkiboo;
mod keypair;
mod cert;
mod ui;

pub use pkiboo::PkiBoo;
pub use ui::Ui;

#[derive(clap::Parser)]
#[command(name="fomega", about="F Omega node management and provisioning utility", version)]
pub struct CliOptions {
    #[command(subcommand)]
    command: Command,

    #[arg(long)]
    db_path: Option<String>
}

#[derive(clap::Subcommand)]
enum Command {
    Media(media::Args),
    Key(keypair::Args),
    Cert(cert::Args)
}

#[tokio::main]
async fn main() {
    let opts = CliOptions::parse();
    let pkiboo = PkiBoo::from_cli_opts(&opts);

    pkiboo.ui().ready().await;
    let result = match &opts.command {
        Command::Media(args) => media::main(&pkiboo, args).await,
        Command::Key(args) => keypair::main(&pkiboo, args).await,
        Command::Cert(args) => cert::main(&pkiboo, args).await
    };

    match result {
        Ok(_) => (),
        Err(e) => println!("Command failed: {}", e)
    }
}
