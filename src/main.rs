use clap::{Parser,Subcommand};

mod pki;
mod cli_common;
mod multihash;

#[derive(clap::Parser)]
#[command(name="fomega", about="F Omega node management and provisioning utility", version)]
struct CliOptions {
    #[command(subcommand)]
    command: Command
}

#[derive(clap::Subcommand)]
enum Command {
    Pki(pki::Args),
}

fn main() {
    let opts = CliOptions::parse();
    let result = match &opts.command {
        Command::Pki(args) => pki::main(&opts, args)
    };
    match result {
        Ok(_) => (),
        Err(e) => println!("Command failed: {}", e)
    }
}
