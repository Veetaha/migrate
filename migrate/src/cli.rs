use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(author)]
pub(crate) enum Args {
    /// Apply the pending migrations
    Up(UpCommand),
    /// Rollback the executed migrations
    Down(DownCommand),
    /// List information about the available migrations
    List,
}

impl Default for Args {
    fn default() -> Self {
        Self::Up(Default::default())
    }
}

#[derive(Debug, StructOpt, Default)]
pub(crate) struct UpCommand {
    #[structopt(flatten)]
    pub(crate) plan: PlanArgGroup,

    /// Name of the bounding migration to be applied the last (inclusive).
    /// By default all the pending migrations will be run upwards.
    #[structopt(long)]
    pub(crate) inclusive_bound: Option<String>,
}

#[derive(Debug, StructOpt)]
pub(crate) struct DownCommand {
    #[structopt(flatten)]
    pub(crate) plan: PlanArgGroup,

    /// Name of the bounding migration to be rolled back the last (inclusive)
    /// This argument is required to prevent sudden deletions of production databases
    #[structopt(long)]
    pub(crate) inclusive_bound: String,
}

#[derive(Debug, StructOpt, Default)]
pub(crate) struct PlanArgGroup {
    /// Don't apply the migrations, only show the list of migrations to be executed
    #[structopt(long, conflicts_with("no_commit"))]
    pub(crate) no_run: bool,

    /// Don't apply the migrations, show the list of migrations to be executed, and also
    /// run the migrations in `NoCommit` mode (no changes will be commited
    /// to the target resource). Works only for migrations that depend on
    /// contexts supporting `NoCommit` mode, migrations that don't will be skipped.
    #[structopt(long)]
    pub(crate) no_commit: bool,
}
