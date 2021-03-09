//! `migrate` cli entrypoint
#![warn(unreachable_pub)]

mod cli;
mod error;

pub use migrate_core as core;

use crate::core::MigrationRunMode;
use error::{DynError, MigrateRunError};
use migrate_core::{MigrationKind, PlanBuilder};
use structopt::StructOpt;

#[derive(Debug)]
pub struct MigrateCli(cli::Args);

impl MigrateCli {
    // TODO: docs
    pub fn from_cli_args() -> Self {
        Self(StructOpt::from_args())
    }

    /// Build the migration context from the cli arguments that the current
    /// process was invoked with. It return an error out if the input cli
    /// arguments are invalid.
    pub fn try_from_cli_args() -> Result<Self, DynError> {
        Ok(Self(StructOpt::from_args_safe()?))
    }

    /// Example of a database migration:
    ///
    /// ```
    /// use async_trait::async_trait;
    /// use migrate::core::{
    ///     Migration, MigrationKind, MigrationRunMode, MigrationCtxProvider, Plan,
    /// };
    ///
    /// /// Database client trait that is used by the migration scripts.
    /// /// It's recommended that we do use a trait here so that we are able
    /// /// to provide two implementations of it (one for real migration and one for
    /// /// running in `no-commit` mode)
    /// #[async_trait]
    /// trait DbClient: Send + Sync {
    ///     // The methods here are very specific to the particular database we migrate
    ///     async fn call_db_api(&self) -> String;
    /// }
    ///
    /// /// Real production database client that commits changes to it!
    /// struct RealDbClient {}
    ///
    /// #[async_trait]
    /// impl DbClient for RealDbClient {
    ///     async fn call_db_api(&self) -> String {
    ///         // Do interaction with the real database
    ///         "<value returned from real database API call here>".to_owned()
    ///     }
    /// }
    ///
    /// // Fake database client that is used for debugging (running migrations in `no-commit` mode)
    /// struct FakeDbClient {}
    ///
    /// #[async_trait]
    /// impl DbClient for FakeDbClient {
    ///     async fn call_db_api(&self) -> String {
    ///         // Probably log some information useful for debugging the migrations here
    ///         tracing::info!("Performing a call to call_db_api...");
    ///
    ///         // Return some dummy value to keep the migrations running
    ///         "<some dummy value, that **pretends** to be returned from the real db api call>".to_owned()
    ///     }
    /// }
    ///
    /// struct DbClientCtxProvider;
    ///
    /// #[async_trait]
    /// impl MigrationCtxProvider for DbClientCtxProvider {
    ///     type Ctx = Box<dyn DbClient>;
    ///
    ///     async fn create_in_commit_mode(self: Box<Self>) -> Result<Self::Ctx, DynError> {
    ///         Ok(Box::new(RealDbClient {}))
    ///     }
    ///
    ///     async fn create_in_no_commit_mode(self: Box<Self>) -> Option<Result<Self::Ctx, DynError>> {
    ///         // We could return `None` here, but it is generally beneficial to spend
    ///         // some time and provide a fake implementation here so the we are able
    ///         // to debug our migrations running them in `no-commit` mode
    ///         Some(Ok(Box::new(FakeDbClient {})))
    ///     }
    /// }
    ///
    /// type DynError = Box<dyn std::error::Error + Send + Sync>;
    ///
    /// struct InitialDbSetupMigration;
    ///
    /// #[async_trait]
    /// impl Migration for InitialDbSetupMigration {
    ///     type Ctx = Box<dyn DbClient>;
    ///
    ///     async fn up(&mut self, db_client: &mut Self::Ctx) -> Result<(), DynError> {
    ///         // Execute database api calls using the database client provided via the
    ///         // context parameter
    ///         db_client.call_db_api().await;
    ///         Ok(())
    ///     }
    ///     async fn down(&mut self, db_client: &mut Self::Ctx) -> Result<(), DynError> {
    ///         // Execute revese database mutations to cancel the changes done in
    ///         // the `up()` method here, you have access to the database client
    ///         // here as well
    /// #       let _ = db_client;
    ///         Ok(())
    ///     }
    /// }
    ///
    /// struct AddNewEntityToDbMigration;
    ///
    /// #[async_trait]
    /// impl Migration for AddNewEntityToDbMigration {
    ///     // The second migration implementation here...
    /// #   type Ctx = Box<dyn DbClient>;
    /// #
    /// #   async fn up(&mut self, db_client: &mut Self::Ctx) -> Result<(), DynError> {
    /// #       Ok(())
    /// #   }
    /// #   async fn down(&mut self, db_client: &mut Self::Ctx) -> Result<(), DynError> {
    /// #       Ok(())
    /// #   }
    /// }
    ///
    /// // Setup or cli main function
    /// #[tokio::main]
    /// async fn main() -> Result<(), DynError> {
    /// #   loop {
    /// #       break;
    ///     let state_storage = migrate_file_state::FileStateLock::new("./migration-state");
    /// #    }
    /// #
    /// #   // Use temporary directory to store the state file in tests
    /// #   let state_file_location = std::env::temp_dir().join("./migration-state");
    /// #   let state_storage = migrate_file_state::FileStateLock::new(&state_file_location);
    /// #
    /// #   struct StateFileGuard(std::path::PathBuf);
    /// #   impl Drop for StateFileGuard {
    /// #       fn drop(&mut self) {
    /// #           if let Err(err) = std::fs::remove_file(&self.0) {
    /// #               eprintln!("Failed to remove state file created in a doctest: {}", err);
    /// #           }
    /// #       }
    /// #   }
    /// #   let _g = StateFileGuard(state_file_location);
    /// #
    ///     let mut plan = Plan::builder(state_storage);
    ///
    ///     plan
    ///         .ctx_provider(DbClientCtxProvider)
    ///         // Add migrations in order one after each other to the plan
    ///         .migration("initial-db-setup", InitialDbSetupMigration)
    ///         .migration("add-new-entity-to-db", AddNewEntityToDbMigration);
    ///
    /// #   // don't run this line, we don't want doc test to read cli args =)
    /// #   loop {
    /// #      break;
    ///     // Run the `migrate` cli to get the parameters of how to
    ///     // build and execute the rest of the migration plan
    ///     migrate::MigrateCli::from_cli_args().run(plan).await?;
    /// #   };
    ///
    ///     // Or use the core api to build and execute the plan
    ///     let plan = plan
    ///         .finish(&MigrationKind::Up {
    ///             inclusive_bound: None,
    ///         }).await?;
    ///
    ///     plan.exec(MigrationRunMode::Commit).await?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn run(self, plan_builder: PlanBuilder) -> Result<(), MigrateRunError> {
        let (cli::PlanArgGroup { no_commit, no_run }, plan) = match self.0 {
            cli::Args::Up(cmd) => {
                let plan = plan_builder
                    .finish(&MigrationKind::Up {
                        inclusive_bound: cmd.inclusive_bound.as_deref(),
                    })
                    .await?;

                (cmd.plan, plan)
            }
            cli::Args::Down(cmd) => {
                let plan = plan_builder
                    .finish(&MigrationKind::Down {
                        inclusive_bound: &cmd.inclusive_bound,
                    })
                    .await?;

                (cmd.plan, plan)
            }
            cli::Args::List => {
                tracing::info!(
                    "Listing registered migrations in order:\n{}",
                    plan_builder.display().finish()
                );
                return Ok(());
            }
        };

        let run_mode = match (no_commit, no_run) {
            (false, false) => MigrationRunMode::Commit,
            (true, false) => MigrationRunMode::NoCommit,
            (false, true) => {
                let plan = plan.display();
                let plan = plan.finish();
                tracing::info!("The following migration plan is generated:\n{}", plan);
                return Ok(());
            }
            (true, true) => unreachable!(
                "BUG: `structopt` should have `conflicts_with` clause that \
                prevents this invalid arguments state"
            ),
        };

        plan.exec(run_mode).await?;

        Ok(())
    }
}
