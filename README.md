[`east`]: https://github.com/okv/east

# :warning: Warning
The crate is in an early MVP stage of development.
You may already use it and it will provide you with a good-enough subset of features,
but for some advanced use cases (e.g. robust migration state locking) may not be covered.

# migrate

`migrate` is a general purpose migration tool.
It provides a flexible interface for writing migration scripts in Rust taking
care of the migration state bookkeeping for you.

## Overview

`migrate` is capable of migrating basically any kind of external state: production databases,
cloud resources, etc.. It monitors what migrations should be applied or rolled back.
With more advanced setup `migrate` prevents data races using external locks ensuring
that only one migration is running at any point of time (currently not implemented yet).

In the basic case you should be able to just implement `up` and (optionally) `down`
methods.

Example boilerplate

```rust
use async_trait::async_trait;
// See the list of ready-to-use contexts bellow, we are happy to expand it!
use migrate::MigrateCli;
use migrate::core::{Migration, Plan, MigrationCtxProvider};
use std::error::Error;
use rusoto_dynamodb::DynamoDb;

// File where `migrate` CLI will record the list of allready applied migrations
// This is called the migration state and there are several different backends
// that implement it. You may also implement your own, e.g. store the migration
// state in your own database.
const MIGRATION_STATE_FILE_PATH: &str = "./migration-state";

type Result<T, E = Box<dyn Error + Send + Sync>> = std::result::Result<T, E>;

struct MyMigration;

#[async_trait]
impl Migration for MyMigration {
    type Ctx = rusoto_dynamodb::DynamoDbClient;

    async fn up(&mut self, ctx: &mut Self::Ctx) -> Result<()> {
        // Apply forward migration logic with the given context
        ctx.put_item(todo!()).await?;
    }

    async fn down(&mut self, ctx: &mut Self::Ctx) -> Result<()> {
        // Rollback the applied migration.
        // Ideally this should be purely inverse to up()
        ctx.delete_item(todo!()).await?;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let state_storage = migrate_state_file::FileStateLock::new(MIGRATION_STATE_FILE_PATH);

    let mut plan = Plan::builder(state_storage);

    plan.ctx_provider(DynamoDbClientProvider)
        // Add migrations in order one after each other to the plan
        .migration("migration-1", MyMigration);

    // Run the `migrate` cli to get the parameters of how to
    // build and execute the rest of the migration plan
    // let cli = migrate::MigrateCli::from_cli_args();

    // Run the CLI (this is run in a test, it's commented out not to run CLI)
    // cli.run(plan).await?;

    Ok(())
}

struct DynamoDbClientProvider;

#[async_trait]
impl MigrationCtxProvider for DynamoDbClientProvider {
    type Ctx = rusoto_dynamodb::DynamoDbClient;

    async fn create_in_commit_mode(self: Box<Self>) -> Result<Self::Ctx> {
        // Create real database client that will do real changes on real data
        todo!()
    }

    async fn create_in_no_commit_mode(self: Box<Self>) -> Option<Result<Self::Ctx>> {
        // We can provide some mocked implementation of database client here
        // via the usage of traits or enums so that the client doesn't commit changes to the database
        todo!()
    }
}
```

## Ready-to-use migration contexts

- DynamoDb: [`migrate_state_dynamodb`](https://docs.rs/migrate_state_dynamodb)
- Local file: [`migrate_state_file`](https://docs.rs/migrate_state_file)

## Locking

`migrate` should support state locking to prevent data races (concurrent migrations).
This feature is not implemented yet, but planned for 1.0 release...

## New migration bootstrapping

`migrate` cli should have a subcommand for creating new migrations stubs
that should be configurable by the migration context trait implementation.
This feature is also planned for 1.0 release but not yet implemented...

## Goals

- Convenient and flexible API
- Consistency

Non-goals:

- Performance

## Contributing

Any contributions are very welcome, just make sure required CI checks pass :D

## References

The idea of `migrate` was initially inspired by [`east`] migration tool (from TypeScript world).

#### License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license, shall be
dually licensed as above, without any additional terms or conditions.
</sub>
