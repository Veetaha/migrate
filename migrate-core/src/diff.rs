use crate::{state::MigrationMeta, DynMigration, PlanBuildErrorKind, PlanBuildError};
use itertools::{EitherOrBoth, Itertools};
use std::mem;
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
        return Err(PlanBuildErrorKind::InconsistentMigrationScripts.into());
    };

    Ok(MigrationsDiff {
        pruned,
        completed,
        pending,
    })
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::*;
    use crate::Migration;
    use async_trait::async_trait;
    use expect_test::expect;
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

    fn dyn_migration_names(dyn_migrations: &[DynMigration]) -> Vec<&str> {
        dyn_migrations.iter().map(|it| it.name.as_str()).collect()
    }

    fn migration_meta_names(migrations_meta: &[MigrationMeta]) -> Vec<&str> {
        migrations_meta.iter().map(|it| it.name.as_str()).collect()
    }

    struct ExpectedDiff(MigrationsDiff);

    impl fmt::Debug for ExpectedDiff {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let MigrationsDiff {
                pruned,
                completed,
                pending,
            } = &self.0;

            return f
                .debug_struct("ExpectedDiff")
                .field("pruned", &migration_meta_names(pruned))
                .field("completed", &dyn_migration_names(completed))
                .field("pending", &dyn_migration_names(pending))
                .finish();
        }
    }

    fn test_diff(
        migrations_saved_in_state: impl IntoIterator<Item = u32>,
        provided_migration_scripts: impl IntoIterator<Item = u32>,
        expected: expect_test::Expect,
    ) {
        let create_name = |id| format!("mig-{}", id);

        let mut migrations_saved_in_state: Vec<_> = migrations_saved_in_state
            .into_iter()
            .map(|i| MigrationMeta {
                name: create_name(i),
            })
            .collect();

        let provided_migration_scripts: Vec<_> = provided_migration_scripts
            .into_iter()
            .map(|i| DynMigration::new(create_name(i), FakeMigration))
            .collect();

        let diff_result = diff(provided_migration_scripts, &mut migrations_saved_in_state);

        if let Ok(MigrationsDiff { completed, .. }) = &diff_result {
            assert_eq!(
                dyn_migration_names(completed),
                migration_meta_names(&migrations_saved_in_state),
            )
        }

        expected.assert_debug_eq(&diff_result.map(ExpectedDiff));
    }

    #[test]
    fn smoke_test() {
        test_diff(
            0..=4,
            2..=6,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [
                            "mig-0",
                            "mig-1",
                        ],
                        completed: [
                            "mig-2",
                            "mig-3",
                            "mig-4",
                        ],
                        pending: [
                            "mig-5",
                            "mig-6",
                        ],
                    },
                )
            "#]],
        );
    }

    #[test]
    fn no_migrations() {
        test_diff(
            0..0,
            0..0,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [],
                        completed: [],
                        pending: [],
                    },
                )
            "#]],
        );
    }

    #[test]
    fn first_migrations() {
        test_diff(
            0..0,
            0..=0,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [],
                        completed: [],
                        pending: [
                            "mig-0",
                        ],
                    },
                )
            "#]]
        );
        test_diff(
            0..0,
            0..=1,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [],
                        completed: [],
                        pending: [
                            "mig-0",
                            "mig-1",
                        ],
                    },
                )
            "#]]
        );
    }

    #[test]
    fn no_diff() {
        test_diff(
            0..=1,
            0..=1,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [],
                        completed: [
                            "mig-0",
                            "mig-1",
                        ],
                        pending: [],
                    },
                )
            "#]],
        );
    }

    #[test]
    fn new_migrations() {
        test_diff(
            0..=1,
            0..=2,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [],
                        completed: [
                            "mig-0",
                            "mig-1",
                        ],
                        pending: [
                            "mig-2",
                        ],
                    },
                )
            "#]]
        );

        test_diff(
            0..=1,
            0..=3,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [],
                        completed: [
                            "mig-0",
                            "mig-1",
                        ],
                        pending: [
                            "mig-2",
                            "mig-3",
                        ],
                    },
                )
            "#]],
        );
    }

    #[test]
    fn pruned_migrations() {
        test_diff(
            0..=2,
            1..=2,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [
                            "mig-0",
                        ],
                        completed: [
                            "mig-1",
                            "mig-2",
                        ],
                        pending: [],
                    },
                )
            "#]]
        );

        test_diff(
            0..=2,
            2..=2,
            expect![[r#"
                Ok(
                    ExpectedDiff {
                        pruned: [
                            "mig-0",
                            "mig-1",
                        ],
                        completed: [
                            "mig-2",
                        ],
                        pending: [],
                    },
                )
            "#]]
        );
    }
}
