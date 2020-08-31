//! `migrate` cli entrypoint

#![warn(unreachable_pub)]

use structopt::StructOpt;

mod cli;

fn main() {
    let _args = cli::Args::from_args();
}
