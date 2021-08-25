//! Implementation of storing the migration state in an [AWS DynamoDB database table][dynamodb].
//!
//! This provides the implementations of traits defined in [`migrate_state`]
//!
//! See [`DdbStateLock`] docs for more details.
//!
//! The following cargo features of the crate are exposed:
//!
//! - `native-tls` (enabled by default) - enables `native-tls` feature in dependent `rusoto` crates
//! - `rustls` - enables `rustls` feature in dependent `rusoto` crates
//!
//! [dynamodb]: https://aws.amazon.com/dynamodb/

#![warn(missing_docs, unreachable_pub, rust_2018_idioms)]
// Makes rustc abort compilation if there are any unsafe blocks in the crate.
// Presence of this annotation is picked up by tools such as cargo-geiger
// and lets them ensure that there is indeed no unsafe code as opposed to
// something they couldn't detect (e.g. unsafe added via macro expansion, etc).
#![forbid(unsafe_code)]

use async_trait::async_trait;
use migrate_state::{Result, StateClient, StateGuard, StateLock};
use rusoto_dynamodb::DynamoDb;
use std::{collections::HashMap, iter};

/// Builder for [`DdbStateLock`] object, see its methods for available configurations.
/// To finish building the object call [`build()`](DdbStateLockBuilder::build) method
pub struct DdbStateLockBuilder(DdbStateCtx);

impl DdbStateLockBuilder {
    /// Override the partition key attribute name used for the stored migration state record.
    ///
    /// Default: `"partition_key"`
    pub fn partition_key_attr_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.0.partition_key_attr.name = name.into();
        self
    }

    /// Override the partition key attribute value used for the stored migration state record.
    ///
    /// Default: `"migrate-state"` (string DynamoDB type)
    pub fn partition_key_attr_val(&mut self, val: rusoto_dynamodb::AttributeValue) -> &mut Self {
        self.0.partition_key_attr.value = val;
        self
    }

    /// Override the sort key attribute name used for the stored migration state record.
    ///
    /// Default: no sort key attribute is added to the record.
    /// If [`sort_key_attr_val`](Self::sort_key_attr_val) was not set, then
    /// it's value will be set to `"migrate-state"` (string DynamoDB type)
    pub fn sort_key_attr_name(&mut self, name: impl Into<String>) -> &mut Self {
        match &mut self.0.sort_key_attr {
            Some(it) => it.name = name.into(),
            None => self.0.sort_key_attr = Some(AttrNameVal::new(name, default_key_attr_value())),
        }
        self
    }

    /// Override the sort key attribute value used for the stored migration state record.
    ///
    /// Default: no sort key attribute is added to the record.
    /// If [`sort_key_attr_name`](Self::sort_key_attr_name) was not set, then
    /// it's value will be set to `"sort_key"`
    pub fn sort_key_attr_val(&mut self, val: rusoto_dynamodb::AttributeValue) -> &mut Self {
        match &mut self.0.sort_key_attr {
            Some(it) => it.value = val,
            None => self.0.sort_key_attr = Some(AttrNameVal::new("sort_key", val)),
        }
        self
    }

    /// Override the payload attribute name used for the stored migration state record.
    ///
    /// Default: `"payload"`
    pub fn payload_attr_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.0.payload_attr_name = name.into();
        self
    }

    /// Consume the builder and return the resulting configured [`DdbStateLock`] object
    pub fn build(self) -> DdbStateLock {
        DdbStateLock(self.0)
    }
}

fn default_key_attr_value() -> rusoto_dynamodb::AttributeValue {
    rusoto_dynamodb::AttributeValue {
        s: Some("migrate-state".into()),
        ..Default::default()
    }
}

/// Implements [`StateLock`] storing the migration state in an [AWS DynamoDB database table][dynamodb].
///
/// <pre class="compile_fail" style="white-space:normal;font:inherit;">
///
/// ⚠️ Warning! State locking is not yet implemented, but it is planned to be implemented.
/// Thus, you have to manually ensure you don't run migrations in parallel in the meantime...
///
/// </pre>
///
/// You can configure how and where the migration state is stored via [`DdbStateLockBuilder`]
/// which is created via [`DdbStateLock::with_builder()`] (or lower-level [`DdbStateLock::builder()`]).
///
/// In general migration state is stored as a single record with a partition key,
/// an optional sort key and the payload attribute of binary array type (payload
/// contains the migration state itself).
///
/// Example usage:
///
/// ```no_run
/// use migrate_state_dynamodb::DdbStateLock;
/// use migrate_core::Plan;
///
/// let ddb_client = rusoto_dynamodb::DynamoDbClient::new(rusoto_core::Region::default());
///
/// let mut state_lock = DdbStateLock::with_builder("ddb-table-name", ddb_client, |it| {
///     // Available configurations.
///     // In this example we pass values that are already set by default just to demo
///     it.partition_key_attr_name("partition_key")
///         .sort_key_attr_name("sort_key")
///         .payload_attr_name("payload")
///         // yeah, `rusoto_dynamodb::AttributeValue` API is a bit ugly...
///         .partition_key_attr_val(rusoto_dynamodb::AttributeValue {
///             s: Some("migrate-state".to_owned()),
///             ..Default::default()
///         })
///         .sort_key_attr_val(rusoto_dynamodb::AttributeValue {
///             s: Some("migrate-state".to_owned()),
///             ..Default::default()
///         })
/// });
///
/// let plan = Plan::builder(state_lock);
/// ```
///
/// [dynamodb]: https://aws.amazon.com/dynamodb/
pub struct DdbStateLock(DdbStateCtx);

impl DdbStateLock {
    /// Returns [`DdbStateLockBuilder`] to configure and create an instance of [`DdbStateLock`].
    ///
    /// Takes two required arguments:
    ///
    /// - `table_name` - The name of the DynamoDB table to store the state in
    /// - `ddb` - [`DynamoDb`] client implementation to use for all DynamoDB API calls
    pub fn builder(
        table_name: impl Into<String>,
        ddb: impl DynamoDb + Send + Sync + 'static,
    ) -> DdbStateLockBuilder {
        DdbStateLockBuilder(DdbStateCtx {
            partition_key_attr: AttrNameVal::new("partition_key", default_key_attr_value()),
            sort_key_attr: None,
            payload_attr_name: "payload".to_owned(),
            table_name: table_name.into(),
            ddb: Box::new(ddb),
        })
    }

    /// Same as [`DdbStateLock::builder()`], but accepts the third argument, which
    /// is a clousure that takes the builder to configure it in a single method call chain.
    /// The method exists only for convenience of creating [`DdbStateLock`] in one expression.
    ///
    /// The return value of the closure is ignored, it is intended only for a single
    /// simple method call chain. Use [`DdbStateLock::builder()`] method to implement
    /// more advanced configuration flow.
    ///
    /// ```
    /// # fn run(ddb_client: rusoto_dynamodb::DynamoDbClient) {
    /// use migrate_state_dynamodb::DdbStateLock;
    ///
    /// let state_lock = DdbStateLock::with_builder("table-name", ddb_client, |it| {
    ///     it.partition_key_attr_name("pk")
    ///         .sort_key_attr_name("sk")
    /// });
    /// # }
    /// ```
    ///
    pub fn with_builder(
        table_name: impl Into<String>,
        ddb: impl DynamoDb + Send + Sync + 'static,
        configure: impl FnOnce(&mut DdbStateLockBuilder) -> &mut DdbStateLockBuilder,
    ) -> Self {
        let mut builder = Self::builder(table_name, ddb);
        let _ = configure(&mut builder);
        builder.build()
    }
}

#[async_trait]
impl StateLock for DdbStateLock {
    async fn lock(self: Box<Self>, _force: bool) -> Result<Box<dyn StateGuard>> {
        // FIXME: acquire the distributed lock here

        Ok(Box::new(DdbStateGuard(DdbStateClient(self.0))))
    }
}

struct DdbStateGuard(DdbStateClient);

#[async_trait]
impl StateGuard for DdbStateGuard {
    fn client(&mut self) -> &mut dyn StateClient {
        &mut self.0
    }

    async fn unlock(mut self: Box<Self>) -> Result<()> {
        // FIXME: release the distributed lock here
        // but be cautios not to corrupt the lock if some other
        // subject has acquired it with `force_lock()`.
        // If that is the case, we should just issue a warning
        // and return successfully
        Ok(())
    }
}

struct DdbStateClient(DdbStateCtx);

#[async_trait]
impl StateClient for DdbStateClient {
    async fn fetch(&mut self) -> Result<Vec<u8>> {
        // FIXME: add retries with exponential backoff
        let item = self
            .0
            .ddb
            .get_item(rusoto_dynamodb::GetItemInput {
                key: self.0.to_primary_key(),
                projection_expression: Some(self.0.payload_attr_name.clone()),
                table_name: self.0.table_name.clone(),
                ..Default::default()
            })
            .await
            .map_err(|source| Error::GetItem { source })?
            .item;

        let mut item = match item {
            Some(it) => it,
            None => return Ok(vec![]),
        };

        let mut payload =
            item.remove(&self.0.payload_attr_name)
                .ok_or_else(|| Error::PayloadAttrNotFound {
                    payload_attr_name: self.0.payload_attr_name.clone(),
                })?;

        let payload = payload.b.take().ok_or(Error::UnexpectedPayloadType {
            actual_value: payload,
        })?;

        Ok(payload.to_vec())
    }

    async fn update(&mut self, state: Vec<u8>) -> Result<()> {
        let state = rusoto_dynamodb::AttributeValue {
            b: Some(state.into()),
            ..Default::default()
        };
        let update_expression = "SET #p = :p";
        let attr_names = iter::once(("#p".to_owned(), self.0.payload_attr_name.clone()));
        let attr_values = iter::once((":p".to_owned(), state));

        self.0
            .ddb
            .update_item(rusoto_dynamodb::UpdateItemInput {
                expression_attribute_names: Some(attr_names.collect()),
                expression_attribute_values: Some(attr_values.collect()),
                key: self.0.to_primary_key(),
                table_name: self.0.table_name.clone(),
                update_expression: Some(update_expression.to_owned()),
                ..Default::default()
            })
            .await
            .map_err(|source| Error::UpdateItem { source })?;

        Ok(())
    }
}

#[derive(Clone)]
struct AttrNameVal {
    name: String,
    value: rusoto_dynamodb::AttributeValue,
}

impl AttrNameVal {
    fn new(name: impl Into<String>, value: rusoto_dynamodb::AttributeValue) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

struct DdbStateCtx {
    partition_key_attr: AttrNameVal,
    sort_key_attr: Option<AttrNameVal>,
    payload_attr_name: String,
    table_name: String,
    ddb: Box<dyn DynamoDb + Send + Sync>,
}

impl DdbStateCtx {
    fn to_primary_key(&self) -> HashMap<String, rusoto_dynamodb::AttributeValue> {
        let partition_key = (
            self.partition_key_attr.name.clone(),
            self.partition_key_attr.value.clone(),
        );
        let sort_key = self
            .sort_key_attr
            .clone()
            .map(|attr| (attr.name, attr.value));

        iter::once(partition_key).chain(sort_key).collect()
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("dynamodb update_item operation failed when updating the migration state")]
    UpdateItem {
        source: rusoto_core::RusotoError<rusoto_dynamodb::UpdateItemError>,
    },

    #[error("dynamodb get_item operation failed when fetching the migration state")]
    GetItem {
        source: rusoto_core::RusotoError<rusoto_dynamodb::GetItemError>,
    },

    #[error(
        "the returned migration state item doesn't contain \
        the payload attribute `{payload_attr_name}`"
    )]
    PayloadAttrNotFound { payload_attr_name: String },

    #[error(
        "the returned migration state item's payload is not \
        of the binary array type, actual value: {actual_value:?}"
    )]
    UnexpectedPayloadType {
        actual_value: rusoto_dynamodb::AttributeValue,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: spin localstack or local dynamodb docker container to test this crate
    #[tokio::test]
    #[ignore]
    async fn smoke_test() {
        let lock = DdbStateLock::builder(
            "veetaha-sandbox",
            rusoto_dynamodb::DynamoDbClient::new(Default::default()),
        )
        .build();

        migrate_state_test::storage(Box::new(lock)).await;
    }
}
