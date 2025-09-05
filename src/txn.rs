use crate::graph::Neo4jConnectionInfo;
use crate::metrics::{Neo4jMetrics, OperationTimer};
use neo4rs::{Query, RowStream, Txn};
use std::sync::Arc;
use tracing::{debug, instrument};

/// Represents an instrumented Neo4j transaction.
///
/// The `InstrumentedTxn` struct wraps around a Neo4j transaction (`Txn`) and includes additional
/// information to facilitate monitoring, instrumentation, and performance analysis. This can
/// be useful for gathering metrics or tracking the transaction's lifecycle within a system.
///
/// # Fields
///
/// * `inner` - The underlying Neo4j transaction object (`Txn`) that this structure wraps around.
/// * `info` - The connection information (`Neo4jConnectionInfo`) associated with this transaction.
/// * `metrics` - An optional shared reference (`Arc`) to the Neo4j metrics collector (`Neo4jMetrics`)
///   for recording runtime monitoring and instrumentation data. If `None`, metrics collection is not performed.
/// * `transaction_timer` - An optional timer (`OperationTimer`) to measure the duration of this transaction
///   for performance profiling. If `None`, timing data is not collected.
///
/// # Usage
/// This struct is designed to enhance visibility into Neo4j transaction operations and provide
/// tools for debugging or optimizing the application's database interactions.
///
/// Note: Ensure that metrics and timing utilities are properly configured if monitoring and
/// profiling are required, as they are optional in this implementation.
pub struct InstrumentedTxn {
    inner: Txn,
    info: Neo4jConnectionInfo,
    metrics: Option<Arc<Neo4jMetrics>>,
    transaction_timer: Option<OperationTimer>,
}

impl InstrumentedTxn {
    /// Create a new instrumented transaction wrapper
    pub(crate) fn new(
        inner: Txn,
        info: Neo4jConnectionInfo,
        metrics: Option<Arc<Neo4jMetrics>>,
    ) -> Self {
        let transaction_timer = metrics.as_ref().map(|_| OperationTimer::start());

        Self {
            inner,
            info,
            metrics,
            transaction_timer,
        }
    }

    /// Execute a query within the transaction and return results
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails within the transaction
    /// context or if there are network connectivity issues.
    #[instrument(
        fields(
            db.system="neo4j",
            db.name=%self.info.database_name,
            db.connection_string=%self.info.connection_string,
            db.version=%self.info.version,
            otel.kind="client",
        ),
        skip(self, query),
        err,
    )]
    pub async fn execute(&mut self, query: Query) -> Result<RowStream, neo4rs::Error> {
        debug!("executing query in transaction");
        self.inner.execute(query).await
    }

    /// Run a query within the transaction without returning results
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails within the transaction
    /// context or if there are network connectivity issues.
    #[instrument(
        fields(
            db.system="neo4j",
            db.name=%self.info.database_name,
            db.connection_string=%self.info.connection_string,
            db.version=%self.info.version,
            otel.kind="client",
        ),
        skip(self, query),
        err,
    )]
    pub async fn run(&mut self, query: Query) -> Result<(), neo4rs::Error> {
        debug!("running query in transaction");
        self.inner.run(query).await
    }

    /// Execute multiple queries sequentially within the transaction
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if any of the queries fail during execution
    /// within the transaction context or if there are network connectivity issues.
    #[instrument(
        fields(
            db.system="neo4j",
            db.name=%self.info.database_name,
            db.connection_string=%self.info.connection_string,
            db.version=%self.info.version,
            otel.kind="client",
            db.operation.batch.size=queries.len(),
        ),
        skip(self, queries),
        err,
    )]
    pub async fn run_queries(&mut self, queries: Vec<Query>) -> Result<(), neo4rs::Error> {
        debug!("running batch queries in transaction");
        self.inner.run_queries(queries).await
    }

    /// Commit the transaction
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the transaction cannot be committed due to
    /// constraint violations, network issues, or other database-related errors.
    #[instrument(
        fields(
            db.system="neo4j",
            db.name=%self.info.database_name,
            db.connection_string=%self.info.connection_string,
            db.version=%self.info.version,
            otel.kind="client",
        ),
        skip(self),
        err,
    )]
    pub async fn commit(mut self) -> Result<(), neo4rs::Error> {
        debug!("committing transaction");

        let result = self.inner.commit().await;

        // Record transaction metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = self.transaction_timer.take() {
                let duration = timer.elapsed();
                metrics.record_transaction_end(duration, result.is_ok(), &self.info.database_name);
            }
        }

        result
    }

    /// Rollback the transaction
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the transaction cannot be rolled back due to
    /// network connectivity issues or other database-related errors.
    #[instrument(
        fields(
            db.system="neo4j",
            db.name=%self.info.database_name,
            db.connection_string=%self.info.connection_string,
            db.version=%self.info.version,
            otel.kind="client",
        ),
        skip(self),
        err,
    )]
    pub async fn rollback(mut self) -> Result<(), neo4rs::Error> {
        debug!("rolling back transaction");

        let result = self.inner.rollback().await;

        // Record transaction metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = self.transaction_timer.take() {
                let duration = timer.elapsed();
                metrics.record_transaction_end(duration, false, &self.info.database_name);
            }
        }

        result
    }

    /// Get a reference to the connection information
    #[must_use]
    pub fn connection_info(&self) -> &Neo4jConnectionInfo {
        &self.info
    }

    /// Get a reference to the underlying transaction
    #[must_use]
    pub fn inner(&self) -> &Txn {
        &self.inner
    }

    /// Get a mutable reference to the underlying transaction
    #[must_use]
    pub fn inner_mut(&mut self) -> &mut Txn {
        &mut self.inner
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_txn_wrapper() {
        let info = crate::graph::Neo4jConnectionInfo {
            database_name: "test".to_string(),
            connection_string: "bolt://localhost:7687".to_string(),
            version: "5.0.0".to_string(),
        };

        assert_eq!(info.database_name, "test");
        assert_eq!(info.connection_string, "bolt://localhost:7687");
        assert_eq!(info.version, "5.0.0");
    }
}
