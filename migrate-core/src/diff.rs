use crate::{state::MigrationMeta, MigrateError, MigrateResult, NamedMigration};
use itertools::{EitherOrBoth, Itertools};
use tracing::error;

pub(crate) struct MigrationsDiff<'a> {
    /// Old migrations removed from the beginning of the history
    pub(crate) pruned: &'a [MigrationMeta],
    /// Completed migrations that are still left in the new migrations list
    pub(crate) completed: &'a [NamedMigration],
    /// New migrations that go after completed migrations in the new list
    pub(crate) pending: &'a [NamedMigration],
}

impl<'a> MigrationsDiff<'a> {
    pub(crate) fn new(
        new_list: &'a [NamedMigration],
        old_list: &'a [MigrationMeta],
    ) -> MigrateResult<MigrationsDiff<'a>> {
        // Find migrations that were removed from the front of the old migrations
        // list and cut them off
        let (pruned, old_list) = old_list.split_at(
            new_list
                .first()
                .and_then(|first_new| old_list.iter().position(|old| old.name == first_new.name))
                .unwrap_or(old_list.len()),
        );

        for it in old_list.iter().zip_longest(new_list) {
            let (old, new) = match it {
                EitherOrBoth::Both(old, new) => {
                    if old.name == new.name {
                        continue;
                    }
                    (&old.name, Some(&new.name))
                }
                EitherOrBoth::Left(old) => (&old.name, None),
                EitherOrBoth::Right(_) => break,
            };

            let new_names = new_list.iter().map(|it| &it.name).format(", ");
            let old_names = old_list.iter().map(|it| &it.name).format(", ");

            let msg = "Configured migration scripts are inconsistent with old applied \
                migrations saved in the state. You should not modify the sequence of \
                migration scripts other than by appending new migration scripts
                or removing old ones from the beggining of the list.";

            match new {
                Some(new) => {
                    error!(
                        %new_names,
                        %old_names,
                        expected_script = old.as_str(),
                        actual_script = new.as_str(),
                        "{}",
                        msg,
                    );
                }
                None => {
                    error!(%new_names, %old_names, missing_script = old.as_str(), "{}", msg);
                }
            }
            return Err(MigrateError::InconsistentMigrationScripts);
        }

        let (completed, pending) = new_list.split_at(old_list.len());

        Ok(MigrationsDiff {
            pruned,
            completed,
            pending,
        })
    }
}
