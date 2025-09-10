use crate::metrics::{Neo4jMetrics, OperationTimer};
use crate::txn::InstrumentedTxn;
use neo4rs::{Graph, Query};
use opentelemetry::metrics::Meter;
use std::ops::Deref;
use std::sync::Arc;
use tracing::{debug, error, info, instrument};

/// A wrapper around Graph that adds tracing instrumentation
pub struct InstrumentedGraph {
    inner: Graph,
    server_address: String,
    server_port: u16,
    metrics: Option<Arc<Neo4jMetrics>>,
}

impl InstrumentedGraph {
    /// Creates a new `InstrumentedGraph` by wrapping an existing Graph
    ///
    /// Uses default Neo4j port 7687 for instrumentation
    #[must_use]
    pub fn new(graph: Graph) -> Self {
        Self {
            inner: graph,
            server_address: "localhost".to_string(),
            server_port: 7687,
            metrics: None,
        }
    }

    /// Adds metrics collection to this instrumented graph
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use neo4rs::Graph;
    /// # use otel_instrumentation_neo4jrs::InstrumentedGraph;
    /// # use opentelemetry::metrics::Meter;
    /// # async fn example(meter: &Meter) -> Result<(), Box<dyn std::error::Error>> {
    /// let graph = InstrumentedGraph::connect(
    ///     "bolt://localhost:7687",
    ///     "neo4j",
    ///     "password"
    /// )
    /// .await?
    /// .with_metrics(meter);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_metrics(mut self, meter: &Meter) -> Self {
        let metrics = Arc::new(Neo4jMetrics::new(meter));
        metrics.increment_connections();
        self.metrics = Some(metrics);
        self
    }

    /// Parses a Neo4j connection URI to extract host and port
    ///
    /// Supports formats like:
    /// - `bolt://localhost:7687`
    /// - `neo4j://host:port`
    /// - `bolt+s://host.com:7687`
    ///
    /// Returns (host, port) with default port 7687 if not specified
    fn parse_neo4j_uri(uri: &str) -> (String, u16) {
        // Remove the protocol prefix (bolt://, neo4j://, etc.)
        let without_protocol = uri.split("://").nth(1).unwrap_or(uri);

        // Remove any authentication info (user:pass@host becomes host)
        let without_auth = if let Some(at_pos) = without_protocol.find('@') {
            &without_protocol[at_pos + 1..]
        } else {
            without_protocol
        };

        // Split host and port
        if let Some(colon_pos) = without_auth.rfind(':') {
            let host = &without_auth[..colon_pos];
            let port_str = &without_auth[colon_pos + 1..];

            // Parse port, default to 7687 if invalid
            let port = port_str.parse::<u16>().unwrap_or(7687);

            (host.to_string(), port)
        } else {
            // No port specified, use default
            (without_auth.to_string(), 7687)
        }
    }

    /// Connects to the database and returns an instrumented graph
    ///
    /// # Errors
    ///
    /// Returns an error if the connection to Neo4j fails
    #[instrument(
        skip(password),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = ?0,  // We'll update this after parsing
            server.port = ?0,     // We'll update this after parsing
            db.operation.name = "connect"
        )
    )]
    pub async fn connect(uri: &str, user: &str, password: &str) -> Result<Self, neo4rs::Error> {
        let (server_address, server_port) = Self::parse_neo4j_uri(uri);

        // Update the span with the parsed values
        tracing::Span::current().record("server.address", server_address.as_str());
        tracing::Span::current().record("server.port", server_port);

        info!(
            "Connecting to Neo4j database at {}:{}",
            server_address, server_port
        );
        match Graph::new(uri, user, password).await {
            Ok(graph) => {
                info!("Successfully connected to database");
                Ok(Self {
                    inner: graph,
                    server_address,
                    server_port,
                    metrics: None,
                })
            }
            Err(e) => {
                error!("Failed to connect to database: {}", e);
                Err(e)
            }
        }
    }

    /// Starts a new transaction on the configured database
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction cannot be started
    #[instrument(
        skip(self),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = "default",
            db.operation.name = "start_transaction"
        )
    )]
    pub async fn start_txn(&self) -> Result<InstrumentedTxn, neo4rs::Error> {
        debug!("Starting transaction on default database");

        // Record transaction start if metrics are enabled
        if let Some(metrics) = &self.metrics {
            metrics.record_transaction_start("default");
        }

        match self.inner.start_txn().await {
            Ok(txn) => {
                info!("Transaction started successfully");
                Ok(InstrumentedTxn::new(
                    txn,
                    self.server_address.clone(),
                    self.server_port,
                    self.metrics.clone(),
                ))
            }
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                Err(e)
            }
        }
    }

    /// Runs a query on the configured database
    ///
    /// # Errors
    ///
    /// Returns an error if the query execution fails
    #[instrument(
        skip(self, q),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = "default",
            db.operation.name = "run"
        )
    )]
    pub async fn run(&self, q: Query) -> Result<(), neo4rs::Error> {
        debug!("Running query");

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.run(q).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(metrics, result.is_ok(), Some("run"), "default");
            }
        }

        match result {
            Ok(()) => {
                info!("Query executed successfully");
                Ok(())
            }
            Err(e) => {
                error!("Query execution failed: {}", e);
                Err(e)
            }
        }
    }

    /// Runs a query on the provided database
    ///
    /// # Errors
    ///
    /// Returns an error if the query execution fails
    #[instrument(
        skip(self, q),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = %db,
            db.operation.name = "run_on"
        )
    )]
    pub async fn run_on(&self, db: &str, q: Query) -> Result<(), neo4rs::Error> {
        debug!("Running query on database: {}", db);

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.run_on(db, q).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(metrics, result.is_ok(), Some("run_on"), db);
            }
        }

        match result {
            Ok(()) => {
                info!("Query executed successfully on database: {}", db);
                Ok(())
            }
            Err(e) => {
                error!("Query execution failed on database {}: {}", db, e);
                Err(e)
            }
        }
    }

    /// Executes a query on the configured database and returns a stream
    ///
    /// # Errors
    ///
    /// Returns an error if the query execution fails
    #[instrument(
        skip(self, q),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = "default",
            db.operation.name = "execute"
        )
    )]
    pub async fn execute(&self, q: Query) -> Result<impl Send, neo4rs::Error> {
        debug!("Executing query");

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.execute(q).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(metrics, result.is_ok(), Some("execute"), "default");
            }
        }

        match result {
            Ok(stream) => {
                info!("Query executed successfully, returning stream");
                Ok(stream)
            }
            Err(e) => {
                error!("Query execution failed: {}", e);
                Err(e)
            }
        }
    }

    /// Executes a query on the provided database and returns a stream
    ///
    /// # Errors
    ///
    /// Returns an error if the query execution fails
    #[instrument(
        skip(self, q),
        fields(
            otel.kind = "CLIENT",
            db.system.name = "neo4j",
            server.address = %self.server_address,
            server.port = %self.server_port,
            db.namespace = %db,
            db.operation.name = "execute_on"
        )
    )]
    pub async fn execute_on(&self, db: &str, q: Query) -> Result<impl Send, neo4rs::Error> {
        debug!("Executing query on database: {}", db);

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.execute_on(db, q).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(metrics, result.is_ok(), Some("execute_on"), db);
            }
        }

        match result {
            Ok(stream) => {
                info!(
                    "Query executed successfully on database: {}, returning stream",
                    db
                );
                Ok(stream)
            }
            Err(e) => {
                error!("Query execution failed on database {}: {}", db, e);
                Err(e)
            }
        }
    }

    /// Get a reference to the inner Graph
    #[must_use]
    pub fn inner(&self) -> &Graph {
        &self.inner
    }

    /// Consume self and return the inner Graph
    #[must_use]
    pub fn into_inner(self) -> Graph {
        self.inner
    }
}

impl Deref for InstrumentedGraph {
    type Target = Graph;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AsRef<Graph> for InstrumentedGraph {
    fn as_ref(&self) -> &Graph {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_neo4j_uri() {
        // Test basic bolt URI
        let (host, port) = InstrumentedGraph::parse_neo4j_uri("bolt://localhost:7687");
        assert_eq!(host, "localhost");
        assert_eq!(port, 7687);

        // Test with authentication
        let (host, port) = InstrumentedGraph::parse_neo4j_uri("bolt://user:pass@example.com:7688");
        assert_eq!(host, "example.com");
        assert_eq!(port, 7688);

        // Test neo4j protocol
        let (host, port) = InstrumentedGraph::parse_neo4j_uri("neo4j://db.example.com:7473");
        assert_eq!(host, "db.example.com");
        assert_eq!(port, 7473);

        // Test without port (should default to 7687)
        let (host, port) = InstrumentedGraph::parse_neo4j_uri("bolt://localhost");
        assert_eq!(host, "localhost");
        assert_eq!(port, 7687);

        // Test bolt+s protocol
        let (host, port) = InstrumentedGraph::parse_neo4j_uri("bolt+s://secure.example.com:7687");
        assert_eq!(host, "secure.example.com");
        assert_eq!(port, 7687);
    }

    #[test]
    fn test_wrapper_creation() {
        // This is a basic test to ensure the wrapper can be created
        // Real tests would require a Neo4j instance
    }
}
