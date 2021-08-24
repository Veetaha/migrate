//! Example migrating a json file that contains an array of user entities
//!
//! The first migration fills the json file with some example user entities
//! of the initial version (they contain only `name` field).
//!
//! The second migration adds the `surname` field to each user.

use std::path::PathBuf;

use async_trait::async_trait;
use migrate::core::{Migration, MigrationCtxProvider, Plan};
use serde::{Deserialize, Serialize};

const JSON_FILE_PATH: &str = "./database.json";
const MIGRATION_STATE_FILE_PATH: &str = "./migration-state";

type DynError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct UserV1 {
    name: String,
}

#[derive(Serialize, Deserialize)]
struct UserV2 {
    name: String,
    surname: String,
}

/// Database client trait that is used by the migration scripts.
/// It's recommended that we do use a trait here so that we are able
/// to provide two implementations of it (one for real migration and one for
/// running in `no-commit` mode)
trait JsonFileClient: Send + Sync {
    fn overwrite(&self, all_entities: Vec<serde_json::Value>) -> Result<(), DynError>;
    fn get_all(&self) -> Result<Vec<serde_json::Value>, DynError>;
}

/// Real production database client that commits changes to it!
struct RealJsonFileClient {
    file_path: PathBuf,
}

impl JsonFileClient for RealJsonFileClient {
    fn overwrite(&self, all_entities: Vec<serde_json::Value>) -> Result<(), DynError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.file_path)?;

        serde_json::to_writer_pretty(file, &all_entities)?;
        Ok(())
    }

    fn get_all(&self) -> Result<Vec<serde_json::Value>, DynError> {
        match std::fs::File::open(&self.file_path) {
            Ok(it) => Ok(serde_json::from_reader(it)?),
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound => Ok(vec![]),
                _ => Err(Box::new(err)),
            },
        }
    }
}

// Fake database client that is used for debugging (running migrations in `no-commit` mode)
struct FakeJsonFileClient;

impl JsonFileClient for FakeJsonFileClient {
    #[tracing::instrument(skip(self))]
    fn overwrite(&self, all_entities: Vec<serde_json::Value>) -> Result<(), DynError> {
        tracing::info!("Pushing to the fake json file...");
        Ok(())
    }

    fn get_all(&self) -> Result<Vec<serde_json::Value>, DynError> {
        tracing::info!("Reading from the fake json file...");
        Ok(vec![])
    }
}

struct DbClientCtxProvider {
    file_path: PathBuf,
}

#[async_trait]
impl MigrationCtxProvider for DbClientCtxProvider {
    type Ctx = Box<dyn JsonFileClient>;

    async fn create_in_commit_mode(self: Box<Self>) -> Result<Self::Ctx, DynError> {
        Ok(Box::new(RealJsonFileClient {
            file_path: self.file_path,
        }))
    }

    async fn create_in_no_commit_mode(self: Box<Self>) -> Option<Result<Self::Ctx, DynError>> {
        // We could return `None` here, but it is generally beneficial to spend
        // some time and provide a fake implementation here so the we are able
        // to debug our migrations running them in `no-commit` mode
        Some(Ok(Box::new(FakeJsonFileClient)))
    }
}

struct Migration1;

fn initial_users() -> Vec<UserV1> {
    vec![
        UserV1 {
            name: "Rarity".to_owned(),
        },
        UserV1 {
            name: "Sweetie".to_owned(),
        },
    ]
}

#[async_trait]
impl Migration for Migration1 {
    type Ctx = Box<dyn JsonFileClient>;

    async fn up(&mut self, client: &mut Self::Ctx) -> Result<(), DynError> {
        // Execute database api calls using the database client provided via the
        // context parameter

        let new_users = initial_users()
            .into_iter()
            .map(|it| serde_json::to_value(it).unwrap());

        let mut existing = client.get_all()?;
        existing.extend(new_users);

        client.overwrite(existing)?;
        Ok(())
    }

    async fn down(&mut self, client: &mut Self::Ctx) -> Result<(), DynError> {
        let mut all_users: Vec<UserV1> = client
            .get_all()?
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<_, _>>()?;

        let initial_users = initial_users();

        all_users.retain(|it| !initial_users.contains(it));

        let all_users = all_users
            .into_iter()
            .map(|it| serde_json::to_value(&it).unwrap())
            .collect();

        client.overwrite(all_users)?;
        Ok(())
    }
}

struct Migration2;

#[async_trait]
impl Migration for Migration2 {
    // The second migration implementation here...
    type Ctx = Box<dyn JsonFileClient>;

    async fn up(&mut self, client: &mut Self::Ctx) -> Result<(), DynError> {
        let users_v1: Vec<UserV1> = client
            .get_all()?
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<_, _>>()?;

        let users_v2 = users_v1
            .into_iter()
            .map(|UserV1 { name }| UserV2 {
                name,
                surname: "<unknown-surname>".to_owned(),
            })
            .map(|it| serde_json::to_value(it).unwrap())
            .collect();

        client.overwrite(users_v2)?;

        Ok(())
    }

    async fn down(&mut self, client: &mut Self::Ctx) -> Result<(), DynError> {
        let users_v2: Vec<UserV2> = client
            .get_all()?
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<_, _>>()?;

        let users_v1 = users_v2
            .into_iter()
            .map(|UserV2 { name, surname: _ }| UserV1 { name })
            .map(|it| serde_json::to_value(it).unwrap())
            .collect();

        client.overwrite(users_v1)?;

        Ok(())
    }
}

// Setup or cli main function
#[tokio::main]
async fn main() -> Result<(), DynError> {
    color_eyre::install().unwrap();
    tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::new()).unwrap();

    try_main()
        .await
        .map_err(|err| color_eyre::eyre::eyre!(err))?;

    Ok(())
}

async fn try_main() -> Result<(), DynError> {
    let state_storage = migrate_state_file::FileStateLock::new(MIGRATION_STATE_FILE_PATH);
    let mut plan = Plan::builder(state_storage);

    plan.ctx_provider(DbClientCtxProvider {
        file_path: JSON_FILE_PATH.into(),
    })
    // Add migrations in order one after each other to the plan
    .migration("migration-1", Migration1)
    .migration("migration-2", Migration2);

    // Run the `migrate` cli to get the parameters of how to
    // build and execute the rest of the migration plan
    migrate::MigrateCli::from_cli_args().run(plan).await?;

    Ok(())
}
