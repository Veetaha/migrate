use structopt::StructOpt;
use strum_macros::EnumString;

#[derive(StructOpt)]
#[structopt(author)]
pub(crate) enum Args {
    /// Apply the pending migrations
    Up(Up),
    /// Rollback the executed migrations
    Down(Down),
}

#[derive(StructOpt)]
pub(crate) struct Up {
    #[structopt(flatten)]
    pub(crate) plan: PlanArgGroup,

    /// Name of the bounding migration to be applied the last (inclusive).
    /// By default all the pending migrations will be run upwards.
    #[structopt(long)]
    pub(crate) inclusive_bound: Option<String>,
}

#[derive(StructOpt)]
pub(crate) struct Down {
    #[structopt(flatten)]
    pub(crate) plan: PlanArgGroup,

    /// Name of the bounding migration to be rolled back the last (inclusive)
    /// This argument is required to prevent sudden deletions of production databases
    #[structopt(long)]
    pub(crate) inclusive_bound: String,
}

#[derive(StructOpt)]
pub(crate) struct PlanArgGroup {
    /// Don't apply the migrations, only show what's going to be performed.
    /// Here, <plan> can be one of two values:
    ///
    /// no-run: Only show the list of migrations to be executed.
    ///
    /// no-commit: Show the list of migrations to be executed, but also
    /// run the migrations in `NoCommit` mode (no changes will be commited
    /// to the target resource). Works only for migrations that depend on
    /// contexts supporting `NoCommit` mode, migrations that don't will be skipped.
    #[structopt(long)]
    pub(crate) plan: Plan,
}

#[derive(EnumString)]
#[strum(serialize_all = "kebab_case")]
pub(crate) enum Plan {
    NoRun,
    NoCommit,
}

impl Default for Plan {
    fn default() -> Plan {
        Plan::NoRun
    }
}
