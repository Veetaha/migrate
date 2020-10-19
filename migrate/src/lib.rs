//! `migrate` cli entrypoint
#![warn(unreachable_pub)]

mod cli;

pub use migrate_core as core;

use anyhow::Result;
use migrate_core::{MigrationKind, PlanBuilder};
use structopt::StructOpt;

pub struct MigrateCtx {
    _private: (),
}

impl MigrateCtx {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub async fn run(plan_builder: PlanBuilder) -> Result<()> {
        // Example:
        // let mut plan = migrate_core::Plan::builder(
        //     migrate_file_state::FileStateLock::new("./state/file/path")
        // );
        // plan
        //     .ctx(val)
        //     .migration("bruh", migr);
        // plan.build().await?.exec()?;

        let args = cli::Args::from_args();

        match args {
            cli::Args::Up(up) => {
                match up.plan.plan {
                    cli::Plan::NoRun => {}
                    cli::Plan::NoCommit => {}
                }

                plan_builder
                    .build(&MigrationKind::Up {
                        inclusive_bound: up.inclusive_bound.as_deref(),
                    })
                    .await?;
            }
            cli::Args::Down(_down) => todo!(),
        }
        Ok(())
    }
}
