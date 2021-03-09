use std::mem;

use crate::{state::MigrationMeta, DynMigration, PlanBuildError};
use itertools::{EitherOrBoth, Itertools};
use tracing::error;

pub(crate) struct MigrationsDiff {
    /// Old migrations removed from the beginning of the history
    pub(crate) pruned: Vec<MigrationMeta>,
    /// Completed migrations that are still left in the new migrations list
    pub(crate) completed: Vec<DynMigration>,
    /// New migrations that go after completed migrations in the new list
    pub(crate) pending: Vec<DynMigration>,
}

pub(crate) fn diff(
    mut new_list: Vec<DynMigration>,
    old_list: &mut Vec<MigrationMeta>,
) -> Result<MigrationsDiff, PlanBuildError> {
    // Find migrations that were removed from the front of the old migrations
    // list and cut them off

    let remaining_old_list = old_list.split_off(
        new_list
            .first()
            .and_then(|first_new| old_list.iter().position(|old| old.name == first_new.name))
            .unwrap_or(0),
    );
    let pruned = mem::replace(old_list, remaining_old_list);

    let mut iter = old_list.iter().zip_longest(&new_list).enumerate();

    let (completed, pending) = loop {
        let (old, new) = match iter.next() {
            None => break (new_list, vec![]),
            Some((i, it)) => match it {
                EitherOrBoth::Both(old, new) => {
                    if old.name == new.name {
                        continue;
                    }
                    (&old.name, Some(&new.name))
                }
                EitherOrBoth::Left(old) => (&old.name, None),
                EitherOrBoth::Right(_) => {
                    let pending = new_list.split_off(i);
                    break (new_list, pending);
                }
            },
        };

        let new_names = new_list.iter().map(|it| &it.name).format(", ");
        let old_names = old_list.iter().map(|it| &it.name).format(", ");

        let msg = "Configured migration scripts are inconsistent with old applied \
            migrations saved in the state. You should not modify the sequence of \
            migration scripts other than by appending new migration scripts \
            or removing old ones from the beggining of the list.";

        match new {
            Some(new) => {
                let actual_script = new.as_str();
                let expected_script = old.as_str();
                error!(%new_names, %old_names, %expected_script, %actual_script, "{}", msg);
            }
            None => {
                error!(%new_names, %old_names, missing_script = old.as_str(), "{}", msg);
            }
        }
        return Err(PlanBuildError::InconsistentMigrationScripts);
    };

    Ok(MigrationsDiff {
        pruned,
        completed,
        pending,
    })
}


#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use crate::Migration;
    use super::*;
    enum Never {}

    struct FakeMigration;
    #[async_trait]
    impl Migration for FakeMigration {
        type Ctx = Never;
        async fn up(&mut self, ctx: &mut Never) -> Result<(), crate::DynError> {
            match *ctx {}
        }
        async fn down(&mut self, ctx: &mut Never) -> Result<(), crate::DynError> {
            match *ctx {}
        }
    }

    #[test]
    fn no_diff() {
        let new_list = vec![
            DynMigration::new("mig-1".to_owned(), FakeMigration),
            DynMigration::new("mig-2".to_owned(), FakeMigration),
        ];
        let mut old_list = vec![
            MigrationMeta { name: "mig-1".to_owned() },
            MigrationMeta { name: "mig-2".to_owned() },
        ];

        let diff = diff(new_list, &mut old_list).unwrap();

        assert_eq!(diff.completed.len(), 2);
        assert_eq!(diff.pending.len(), 0);
        assert_eq!(diff.pending.len(), 0);
    }
}
