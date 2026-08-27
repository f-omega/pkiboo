use clap::Parser;

mod cert;
mod cli_common;
mod backup;
mod keypair;
mod media;
mod multihash;
mod paper;
mod pkiboo;
mod status;
mod ui;
mod util;

pub use pkiboo::PkiBoo;
pub use ui::Ui;

#[derive(clap::Parser)]
#[command(
    name = "fomega",
    about = "F Omega node management and provisioning utility",
    version
)]
pub struct CliOptions {
    #[command(subcommand)]
    command: Command,

    #[arg(long)]
    db_path: Option<String>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Show the health of pkiboo's managed state
    Status(status::Args),

    /// Create, inspect, verify, and manage storage media
    Media(media::Args),

    /// Create, back up, verify, and manage private keys
    Key(keypair::Args),

    /// Create, inspect, export, and manage certificates
    Cert(cert::Args),

    /// Inspect and manage printable recovery artifacts
    Paper(paper::Args),

    /// Copy the database recovery hint onto one or more media
    Backup(backup::Args),

}

#[tokio::main]
async fn main() {
    let opts = CliOptions::parse();
    let pkiboo = PkiBoo::from_cli_opts(&opts);

    pkiboo.ui().ready().await;
    let result = match &opts.command {
        Command::Status(args) => status::main(&pkiboo, args).await,
        Command::Media(args) => media::main(&pkiboo, args).await,
        Command::Key(args) => keypair::main(&pkiboo, args).await,
        Command::Cert(args) => cert::main(&pkiboo, args).await,
        Command::Paper(args) => paper::main(&pkiboo, args).await,
        Command::Backup(args) => backup::main(&pkiboo, args).await,
    };

    match result {
        Ok(_) => (),
        Err(e) => println!("Command failed: {}", e),
    }
}
