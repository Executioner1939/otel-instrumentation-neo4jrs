use crate::graph::Neo4jConnectionInfo;
use crate::metrics::{Neo4jMetrics, OperationTimer};
use neo4rs::{Query, RowStream, Txn};
use std::sync::Arc;
use tracing::{debug, instrument};

/// An instrumented wrapper around `neo4rs::Txn` that adds OpenTelemetry tracing
///
/// This struct provides the same API as `neo4rs::Txn` but adds comprehensive
/// OpenTelemetry instrumentation following database semantic conventions.
///
/// Transactions create a span hierarchy where the transaction itself is a parent
/// span and individual operations within the transaction are child spans.
///
/// # Example
///
/// ```
/// use otel_instrumentation_neo4jrs::InstrumentedGraph;
/// use neo4rs::{ConfigBuilder, query};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let graph = InstrumentedGraph::new("bolt://localhost:7687", "neo4j", "password").await?;
/// let mut txn = graph.start_txn().await?;
///
/// txn.run(query("CREATE (n:Person {name: 'Alice'})")).await?;
/// txn.run(query("CREATE (n:Person {name: 'Bob'})")).await?;
///
/// txn.commit().await?;
/// # Ok(())
/// # }
/// ```
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
            server.address=%self.info.server_address,
            server.port=self.info.server_port,
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
            server.address=%self.info.server_address,
            server.port=self.info.server_port,
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
            server.address=%self.info.server_address,
            server.port=self.info.server_port,
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
            server.address=%self.info.server_address,
            server.port=self.info.server_port,
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
            server.address=%self.info.server_address,
            server.port=self.info.server_port,
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
            server_address: "localhost".to_string(),
            server_port: 7687,
            version: "5.0.0".to_string(),
        };

        assert_eq!(info.database_name, "test");
        assert_eq!(info.server_address, "localhost");
        assert_eq!(info.server_port, 7687);
        assert_eq!(info.version, "5.0.0");
    }
}
