[`east`]: https://github.com/okv/east

# :warning: Warning
The crate is in an early stage of development. Code snippets bellow don't work,
they are just the scratch of the desired API.

# migrate

`migrate` is a general purpose migration tool.
It provides a flexible interface for writing migration scripts in Rust taking
care of the state bookkeeping for you.

## Overview

`migrate` is capable of migrating basically any kind of external state: production databases,
cloud resources, etc.. It monitors what migrations should be applied or rolled back.
With more advanced setup `migrate` prevents data races using external locks ensuring
that only one migration is running at any point of time.

In the basic case you should be able to just implement `up` and (optionally) `down`
methods.

Example boilerplate

```rust
use async_trait::async_trait;
// See the list of ready-to-use contexts bellow, we are happy to expand it!
use migrate_dynamodb::{Context, StateStorage};
use migrate::{Migration, MigrateApp};
use std::error::Error;

type Result<T, E> = std::result::Result<T, Box<dyn Error>>;

struct MyMigration;

#[async_trait]
impl Migration for MyMigration {
    type Context = Context;

    async fn up(self, ctx: Context) -> Result<()> {
        // Apply forward migration logic
        let client: &dyn rusoto_dynamodb::DynamoDb = &ctx.client;
    }

    async fn down(self, ctx: Context) -> Result<()> {
        // Rollback the applied migration.
        // Ideally this should be purely inverse to up()
    }
}

#[tokio::main]
async fn main() {
    // This needs more design effort...
    let app = MigrateApp::new::<StateStorage>(
        vec![MyMigration],
    );
    app.run_cli().await.unwrap();
}
```

## Ready-to-use migration contexts

- DynamoDb: `migrate_dynamodb` (yet to be implemented...)

## Locking

`migrate` should support state locking to prevent data races (concurrent migrations).
This features is not implemented yet, but planned for 1.0 release...

## New migration bootstrapping

`migrate` cli should have a subcommand for creating new migrations stubs
that should be configurable by the migration context trait implementation.
This feature is also planned for 1.0 release...

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
