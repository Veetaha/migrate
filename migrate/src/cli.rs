use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(author)]
pub(crate) enum Args {
    /// Apply the pending migrations
    Up(Up),
    /// Rollback the migrations
    Down(Down),
}

#[derive(StructOpt)]
pub(crate) struct Up {
    /// Name of the bounding migration to be applied the last (inclusive)
    to: Option<String>,
}

#[derive(StructOpt)]
pub(crate) struct Down {
    /// Name of the bounding migration to be rolled back the last (inclusive)
    /// This argument is required to preven sudden deletions of production databases
    to: String,
}
