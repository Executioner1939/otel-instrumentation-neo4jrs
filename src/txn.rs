use crate::metrics::{Neo4jMetrics, OperationTimer};
use neo4rs::{Query, RowStream, Txn};
use std::sync::Arc;
use tracing::{debug, error, info, instrument};

/// An instrumented wrapper around Neo4j transaction
pub struct InstrumentedTxn {
    inner: Txn,
    server_address: String,
    server_port: u16,
    metrics: Option<Arc<Neo4jMetrics>>,
    transaction_timer: Option<OperationTimer>,
}

impl InstrumentedTxn {
    /// Create a new instrumented transaction wrapper
    #[must_use]
    pub fn new(
        inner: Txn,
        server_address: String,
        server_port: u16,
        metrics: Option<Arc<Neo4jMetrics>>,
    ) -> Self {
        // Start transaction timer if metrics are enabled
        let transaction_timer = metrics.as_ref().map(|_| OperationTimer::start());

        Self {
            inner,
            server_address,
            server_port,
            metrics,
            transaction_timer,
        }
    }

    /// Execute a query within the transaction and return results
    ///
    /// # Errors
    ///
    /// Returns an error if the query execution fails
    #[instrument(
        skip(self, query),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = "default",
            db.operation.name = "txn_execute"
        ),
        err
    )]
    pub async fn execute(&mut self, query: Query) -> Result<RowStream, neo4rs::Error> {
        debug!("Executing query in transaction");

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.execute(query).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(metrics, result.is_ok(), Some("txn_execute"), "default");
            }
        }

        match result {
            Ok(stream) => {
                info!("Query executed successfully in transaction");
                Ok(stream)
            }
            Err(e) => {
                error!("Query execution failed in transaction: {}", e);
                Err(e)
            }
        }
    }

    /// Run a query within the transaction without returning results
    ///
    /// # Errors
    ///
    /// Returns an error if the query execution fails
    #[instrument(
        skip(self, query),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = "default",
            db.operation.name = "txn_run"
        ),
        err
    )]
    pub async fn run(&mut self, query: Query) -> Result<(), neo4rs::Error> {
        debug!("Running query in transaction");

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.run(query).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(metrics, result.is_ok(), Some("txn_run"), "default");
            }
        }

        match result {
            Ok(()) => {
                info!("Query run successfully in transaction");
                Ok(())
            }
            Err(e) => {
                error!("Query run failed in transaction: {}", e);
                Err(e)
            }
        }
    }

    /// Execute multiple queries sequentially within the transaction
    ///
    /// # Errors
    ///
    /// Returns an error if any query execution fails
    #[instrument(
        skip(self, queries),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = "default",
            db.operation.name = "txn_run_queries",
            db.operation.batch.size = queries.len()
        ),
        err
    )]
    pub async fn run_queries(&mut self, queries: Vec<Query>) -> Result<(), neo4rs::Error> {
        debug!("Running {} queries in transaction", queries.len());

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.run_queries(queries).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ =
                    timer.record_query(metrics, result.is_ok(), Some("txn_run_queries"), "default");
            }
        }

        match result {
            Ok(()) => {
                info!("Batch queries run successfully in transaction");
                Ok(())
            }
            Err(e) => {
                error!("Batch queries failed in transaction: {}", e);
                Err(e)
            }
        }
    }

    /// Commit the transaction
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction cannot be committed
    #[instrument(
        skip(self),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = "default",
            db.operation.name = "txn_commit"
        ),
        err
    )]
    pub async fn commit(self) -> Result<(), neo4rs::Error> {
        debug!("Committing transaction");

        let result = self.inner.commit().await;

        // Record transaction end if metrics enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = self.transaction_timer {
                let duration = timer.elapsed();
                metrics.record_transaction_end(duration, result.is_ok(), "default");
            }
        }

        match result {
            Ok(()) => {
                info!("Transaction committed successfully");
                Ok(())
            }
            Err(e) => {
                error!("Transaction commit failed: {}", e);
                Err(e)
            }
        }
    }

    /// Rollback the transaction
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction cannot be rolled back
    #[instrument(
        skip(self),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = "default",
            db.operation.name = "txn_rollback"
        ),
        err
    )]
    pub async fn rollback(self) -> Result<(), neo4rs::Error> {
        debug!("Rolling back transaction");

        let result = self.inner.rollback().await;

        // Record transaction end if metrics enabled (rollback = not committed)
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = self.transaction_timer {
                let duration = timer.elapsed();
                metrics.record_transaction_end(duration, false, "default");
            }
        }

        match result {
            Ok(()) => {
                info!("Transaction rolled back successfully");
                Ok(())
            }
            Err(e) => {
                error!("Transaction rollback failed: {}", e);
                Err(e)
            }
        }
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
