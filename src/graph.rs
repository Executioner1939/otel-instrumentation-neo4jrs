//! An instrumented wrapper around `neo4rs::Graph` that adds OpenTelemetry tracing.
//!
//! This module provides methods for connecting to a Neo4j database,
//! executing queries, and attaching OpenTelemetry instruments following
//! database semantic conventions. It serves to enhance the usability of the
//! `neo4rs` crate while embedding observability within a distributed system.
//!
//! # Key Features
//!
//! - Establishes a connection to Neo4j with OpenTelemetry tracing metadata
//! - Supports executing queries with embedded spans for observability
//! - Adheres to OpenTelemetry database semantic conventions
//! - Handles errors gracefully while converting custom errors into Neo4j errors
//!
//! ## Dependencies
//!
//! - `neo4rs`: Used for creating connections and executing queries
//! - `tracing`: Used for OpenTelemetry instrumentation
//!
//! # Example Usage
//!
//! ```rust,no_run
//! use otel_instrumentation_neo4jrs::InstrumentedGraph;
//! use neo4rs::query;
//! use opentelemetry::global;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Get tracer and meter from your configured providers
//!     let tracer = global::tracer("my-service");
//!     let meter = global::meter("my-service");
//!
//!     // Create instrumented connection using builder
//!     let graph = InstrumentedGraph::builder()
//!         .with_tracer(tracer)
//!         .with_meter(meter)
//!         .build()
//!         .connect("bolt://localhost:7687", "neo4j", "password")
//!         .await?;
//!
//!     // Execute queries with instrumentation
//!     graph.execute(query("MATCH (n) RETURN count(n) as total")).await?;
//!     Ok(())
//! }
//! ```
//!
//! # Error Handling
//!
//! Most methods return `neo4rs::Error` on failure, which could arise from:
//!
//! - Issues connecting to the Neo4j database
//! - Query execution failures
//! - Network connectivity problems
use crate::builder::InstrumentedGraphBuilder;
use crate::error::{InstrumentationError, InstrumentationResult};
use crate::metrics::{Neo4jMetrics, OperationTimer};
use crate::telemetry::TelemetryConfig;
use neo4rs::{Graph, Query};
use opentelemetry::global::BoxedTracer;
use opentelemetry::metrics::Meter;
use opentelemetry::trace::{Span, SpanKind, Status, Tracer};
use opentelemetry::KeyValue;
use opentelemetry_semantic_conventions::attribute::{
    DB_NAMESPACE, DB_OPERATION_NAME, DB_SYSTEM_NAME, SERVER_ADDRESS, SERVER_PORT,
};
use std::ops::Deref;
use std::sync::Arc;
use tracing::{debug, warn};

/// Connection information retrieved from Neo4j server.
#[derive(Clone, Debug)]
pub struct Neo4jConnectionInfo {
    pub database_name: String,
    pub version: String,
    pub connection_string: String, // Store sanitized connection URI for tracing
    pub server_address: String,    // Parsed server address (hostname/IP)
    pub server_port: Option<i64>,  // Parsed server port (if present)
}

/// Configuration for creating an instrumented Neo4j connection.
///
/// This struct is created by the builder and holds the telemetry configuration
/// until a connection is established.
pub struct InstrumentedGraphConfig {
    tracer: Option<BoxedTracer>,
    meter: Option<Meter>,
}

impl InstrumentedGraphConfig {
    /// Creates a new configuration with the given telemetry providers.
    pub(crate) fn new(tracer: Option<BoxedTracer>, meter: Option<Meter>) -> Self {
        Self { tracer, meter }
    }

    /// Connects to Neo4j using the same signature as neo4rs `Graph::new()`.
    ///
    /// # Arguments
    ///
    /// * `uri` - The Neo4j connection URI (e.g., `bolt://localhost:7687`)
    /// * `user` - The username for authentication
    /// * `password` - The password for authentication
    ///
    /// # Errors
    ///
    /// Returns an error if the connection to Neo4j fails.
    pub async fn connect(
        self,
        uri: &str,
        user: &str,
        password: &str,
    ) -> Result<InstrumentedGraph, neo4rs::Error> {
        let telemetry = TelemetryConfig::with_providers(self.tracer, self.meter);
        InstrumentedGraph::with_telemetry_config(uri, user, password, telemetry).await
    }
}

/// An instrumented wrapper around `neo4rs::Graph` that adds OpenTelemetry tracing.
///
/// This struct provides the same API as `neo4rs::Graph` but adds comprehensive
/// OpenTelemetry instrumentation following database semantic conventions.
///
/// Use the builder pattern to create instrumented connections with your telemetry providers.
///
/// # Example
///
/// ```rust,ignore
/// let graph = InstrumentedGraph::builder()
///     .with_tracer(my_tracer)
///     .with_meter(my_meter)
///     .build()
///     .connect("bolt://localhost:7687", "neo4j", "password")
///     .await?;
/// ```
pub struct InstrumentedGraph {
    inner: Graph,
    info: Neo4jConnectionInfo,
    tracer: Option<BoxedTracer>,
    metrics: Option<Arc<Neo4jMetrics>>,
}

impl InstrumentedGraph {
    /// Creates a new builder for configuring an instrumented Neo4j connection.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let graph = InstrumentedGraph::builder()
    ///     .with_tracer(my_tracer)
    ///     .with_meter(my_meter)
    ///     .build()
    ///     .connect("bolt://localhost:7687", "neo4j", "password")
    ///     .await?;
    /// ```
    #[must_use]
    pub fn builder() -> InstrumentedGraphBuilder {
        InstrumentedGraphBuilder::new()
    }

    /// Creates an instrumented graph with a telemetry configuration.
    ///
    /// This is used internally by the builder and other convenience methods.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the connection or info retrieval fails.
    pub(crate) async fn with_telemetry_config(
        uri: &str,
        user: &str,
        password: &str,
        config: TelemetryConfig,
    ) -> Result<Self, neo4rs::Error> {
        let graph = Graph::new(uri, user, password).await?;
        let info = Self::get_connection_info(&graph, uri)
            .await
            .map_err(Self::convert_instrumentation_error)?;

        let metrics = if let Some(meter) = config.meter {
            let metrics = Arc::new(Neo4jMetrics::new(&meter));
            metrics.increment_connections();
            Some(metrics)
        } else {
            None
        };

        Ok(Self {
            inner: graph,
            info,
            tracer: config.tracer,
            metrics,
        })
    }

    /// Helper to convert instrumentation errors to neo4rs errors for API compatibility.
    fn convert_instrumentation_error(e: InstrumentationError) -> neo4rs::Error {
        if let InstrumentationError::Neo4jError(neo4j_err) = e {
            neo4j_err
        } else {
            // Log the instrumentation error and return a generic IO error
            warn!(
                "Instrumentation error while retrieving connection info: {}",
                e
            );
            neo4rs::Error::IOError {
                detail: std::io::Error::other("Failed to retrieve connection information"),
            }
        }
    }

    /// Executes a query and returns results as a stream.
    ///
    /// This method instruments the query execution with OpenTelemetry spans
    /// following database semantic conventions.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails or if there are
    /// network connectivity issues with the Neo4j server.
    pub async fn execute(&self, query: Query) -> Result<impl Send, neo4rs::Error> {
        debug!("executing neo4j query");

        // Create span if tracer is available
        let mut span = self.tracer.as_ref().map(|tracer| {
            let mut attributes = vec![
                KeyValue::new(DB_SYSTEM_NAME, "neo4j"),
                KeyValue::new(DB_NAMESPACE, self.info.database_name.clone()),
                KeyValue::new(SERVER_ADDRESS, self.info.server_address.clone()),
                KeyValue::new(DB_OPERATION_NAME, "execute"),
            ];

            if let Some(port) = self.info.server_port {
                attributes.push(KeyValue::new(SERVER_PORT, port));
            }

            tracer
                .span_builder("neo4j.query")
                .with_kind(SpanKind::Client)
                .with_attributes(attributes)
                .start(tracer)
        });

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.execute(query).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(
                    metrics,
                    result.is_ok(),
                    Some("execute"),
                    &self.info.database_name,
                );
            }
        }

        // Set span status based on result
        if let Some(ref mut span) = span {
            match &result {
                Ok(_) => span.set_status(Status::Ok),
                Err(e) => {
                    span.record_error(e);
                    span.set_status(Status::error(format!("Query execution failed: {e}")));
                }
            }
            span.end();
        }

        result
    }

    /// Runs a query without returning results.
    ///
    /// This method instruments the query execution with OpenTelemetry spans.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails or if there are
    /// network connectivity issues with the Neo4j server.
    pub async fn run(&self, query: Query) -> Result<(), neo4rs::Error> {
        debug!("running neo4j query without results");

        // Create span if tracer is available
        let mut span = self.tracer.as_ref().map(|tracer| {
            let mut attributes = vec![
                KeyValue::new(DB_SYSTEM_NAME, "neo4j"),
                KeyValue::new(DB_NAMESPACE, self.info.database_name.clone()),
                KeyValue::new(SERVER_ADDRESS, self.info.server_address.clone()),
                KeyValue::new(DB_OPERATION_NAME, "run"),
            ];

            if let Some(port) = self.info.server_port {
                attributes.push(KeyValue::new(SERVER_PORT, port));
            }

            tracer
                .span_builder("neo4j.run")
                .with_kind(SpanKind::Client)
                .with_attributes(attributes)
                .start(tracer)
        });

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.run(query).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(
                    metrics,
                    result.is_ok(),
                    Some("run"),
                    &self.info.database_name,
                );
            }
        }

        // Set span status based on result
        if let Some(ref mut span) = span {
            match &result {
                Ok(()) => span.set_status(Status::Ok),
                Err(e) => {
                    span.record_error(e);
                    span.set_status(Status::error(format!("Query run failed: {e}")));
                }
            }
            span.end();
        }

        result
    }

    /// Executes a query on a specific database.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails, if the specified
    /// database does not exist, or if there are network connectivity issues.
    pub async fn execute_on(
        &self,
        database: &str,
        query: Query,
    ) -> Result<impl Send, neo4rs::Error> {
        debug!("executing neo4j query on database {}", database);

        // Create span if tracer is available
        let mut span = self.tracer.as_ref().map(|tracer| {
            let mut attributes = vec![
                KeyValue::new(DB_SYSTEM_NAME, "neo4j"),
                KeyValue::new(DB_NAMESPACE, database.to_string()),
                KeyValue::new(SERVER_ADDRESS, self.info.server_address.clone()),
                KeyValue::new(DB_OPERATION_NAME, "execute_on"),
            ];

            if let Some(port) = self.info.server_port {
                attributes.push(KeyValue::new(SERVER_PORT, port));
            }

            tracer
                .span_builder("neo4j.execute_on")
                .with_kind(SpanKind::Client)
                .with_attributes(attributes)
                .start(tracer)
        });

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.execute_on(database, query).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(metrics, result.is_ok(), Some("execute_on"), database);
            }
        }

        // Set span status based on result
        if let Some(ref mut span) = span {
            match &result {
                Ok(_) => span.set_status(Status::Ok),
                Err(e) => {
                    span.record_error(e);
                    span.set_status(Status::error(format!(
                        "Query execution on {database} failed: {e}"
                    )));
                }
            }
            span.end();
        }

        result
    }

    /// Runs a query on a specific database without returning results.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails, if the specified
    /// database does not exist, or if there are network connectivity issues.
    pub async fn run_on(&self, database: &str, query: Query) -> Result<(), neo4rs::Error> {
        debug!(
            "running neo4j query on database {} without results",
            database
        );

        // Create span if tracer is available
        let mut span = self.tracer.as_ref().map(|tracer| {
            let mut attributes = vec![
                KeyValue::new(DB_SYSTEM_NAME, "neo4j"),
                KeyValue::new(DB_NAMESPACE, database.to_string()),
                KeyValue::new(SERVER_ADDRESS, self.info.server_address.clone()),
                KeyValue::new(DB_OPERATION_NAME, "run_on"),
            ];

            if let Some(port) = self.info.server_port {
                attributes.push(KeyValue::new(SERVER_PORT, port));
            }

            tracer
                .span_builder("neo4j.run_on")
                .with_kind(SpanKind::Client)
                .with_attributes(attributes)
                .start(tracer)
        });

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = self.inner.run_on(database, query).await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(metrics, result.is_ok(), Some("run_on"), database);
            }
        }

        // Set span status based on result
        if let Some(ref mut span) = span {
            match &result {
                Ok(()) => span.set_status(Status::Ok),
                Err(e) => {
                    span.record_error(e);
                    span.set_status(Status::error(format!(
                        "Query run on {database} failed: {e}"
                    )));
                }
            }
            span.end();
        }

        result
    }

    /// Starts a new transaction.
    ///
    /// This method creates an instrumented transaction wrapper.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the transaction cannot be started due to
    /// connection issues or server-side constraints.
    pub async fn start_txn(&self) -> Result<crate::txn::InstrumentedTxn, neo4rs::Error> {
        debug!("starting neo4j transaction");

        // Create span if tracer is available
        let mut span = self.tracer.as_ref().map(|tracer| {
            let mut attributes = vec![
                KeyValue::new(DB_SYSTEM_NAME, "neo4j"),
                KeyValue::new(DB_NAMESPACE, self.info.database_name.clone()),
                KeyValue::new(SERVER_ADDRESS, self.info.server_address.clone()),
                KeyValue::new(DB_OPERATION_NAME, "start_transaction"),
            ];

            if let Some(port) = self.info.server_port {
                attributes.push(KeyValue::new(SERVER_PORT, port));
            }

            tracer
                .span_builder("neo4j.transaction.start")
                .with_kind(SpanKind::Client)
                .with_attributes(attributes)
                .start(tracer)
        });

        // Record transaction start if metrics are enabled
        if let Some(metrics) = &self.metrics {
            metrics.record_transaction_start(&self.info.database_name);
        }

        let result = self.inner.start_txn().await;

        // Set span status based on result
        if let Some(ref mut span) = span {
            match &result {
                Ok(_) => span.set_status(Status::Ok),
                Err(e) => {
                    span.record_error(e);
                    span.set_status(Status::error(format!("Failed to start transaction: {e}")));
                }
            }
            span.end();
        }

        result.map(|txn| {
            crate::txn::InstrumentedTxn::new(txn, self.info.clone(), self.metrics.clone())
        })
    }

    /// Returns a reference to the Neo4j connection information.
    ///
    /// # Returns
    ///
    /// A reference to a [`Neo4jConnectionInfo`] instance containing details about the Neo4j connection.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let connection_info = graph.connection_info();
    /// println!("Database: {}", connection_info.database_name);
    /// println!("Version: {}", connection_info.version);
    /// ```
    #[must_use]
    pub fn connection_info(&self) -> &Neo4jConnectionInfo {
        &self.info
    }

    /// Returns a reference to the inner `Graph` instance.
    ///
    /// This provides direct access to the underlying `neo4rs::Graph` for cases
    /// where instrumentation is not needed or for accessing methods not yet
    /// wrapped by this instrumented version.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let inner_graph = instrumented_graph.inner();
    /// // Use inner_graph for direct neo4rs operations
    /// ```
    #[must_use]
    pub fn inner(&self) -> &Graph {
        &self.inner
    }
}

impl InstrumentedGraph {
    /// Parses a connection URI to extract server address and port.
    ///
    /// # Arguments
    ///
    /// * `uri` - The connection URI to parse
    ///
    /// # Returns
    ///
    /// A tuple of (`server_address`, `optional_port`).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let (address, port) = parse_connection_uri("bolt://localhost:7687");
    /// assert_eq!(address, "localhost");
    /// assert_eq!(port, Some(7687));
    /// ```
    fn parse_connection_uri(uri: &str) -> (String, Option<i64>) {
        // Parse the URI to extract server address and port
        // Format is usually bolt://user:password@host:port or bolt://host:port

        let mut clean_uri = uri.to_string();

        // Remove the protocol prefix
        if let Some(proto_end) = clean_uri.find("://") {
            clean_uri = clean_uri[proto_end + 3..].to_string();
        }

        // Remove credentials if present (user:pass@)
        if let Some(at_pos) = clean_uri.find('@') {
            clean_uri = clean_uri[at_pos + 1..].to_string();
        }

        // Now we should have host:port or just host
        if let Some(colon_pos) = clean_uri.rfind(':') {
            let host = clean_uri[..colon_pos].to_string();
            let port_str = &clean_uri[colon_pos + 1..];

            // Try to parse the port
            let port = port_str.parse::<i64>().ok();
            (host, port)
        } else {
            // No port specified
            (clean_uri, None)
        }
    }

    /// Sanitizes a connection URI to remove sensitive information like passwords.
    ///
    /// # Arguments
    ///
    /// * `uri` - The connection URI to sanitize
    ///
    /// # Returns
    ///
    /// A sanitized connection string with the password replaced by `****`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sanitized = sanitize_connection_string("bolt://user:password@localhost:7687");
    /// assert_eq!(sanitized, "bolt://user:****@localhost:7687");
    /// ```
    fn sanitize_connection_string(uri: &str) -> String {
        // Remove password from connection string for tracing
        // Format is usually bolt://user:password@host:port or bolt://host:port
        if let Some(at_pos) = uri.find('@') {
            if let Some(proto_end) = uri.find("://") {
                let protocol = &uri[..proto_end + 3];
                let after_at = &uri[at_pos..];
                // Check if there's a username (has colon before @)
                if let Some(colon_pos) = uri[proto_end + 3..at_pos].find(':') {
                    let username = &uri[proto_end + 3..proto_end + 3 + colon_pos];
                    format!("{protocol}{username}:****{after_at}")
                } else {
                    // No password, return as-is
                    uri.to_string()
                }
            } else {
                uri.to_string()
            }
        } else {
            uri.to_string()
        }
    }

    /// Retrieves connection information from a Neo4j database.
    ///
    /// This asynchronous function fetches relevant information about the Neo4j database,
    /// including its version, current database name, and sanitized connection string.
    /// If certain details cannot be retrieved, it falls back to default values with
    /// appropriate warning logs.
    ///
    /// # Arguments
    ///
    /// * `graph` - A reference to a `Graph` instance used to interact with the Neo4j database
    /// * `uri` - The connection URI string for sanitization and storage
    ///
    /// # Returns
    ///
    /// An `InstrumentationResult` containing a `Neo4jConnectionInfo` struct populated with:
    ///
    /// - `database_name` - The name of the current Neo4j database (defaults to "neo4j" if retrieval fails)
    /// - `version` - The Neo4j version (defaults to "unknown" if retrieval fails)
    /// - `connection_string` - The sanitized connection URI with password masked
    ///
    /// # Behavior
    ///
    /// The function performs the following steps:
    ///
    /// 1. Queries the Neo4j database using `dbms.components()` to fetch the database version
    /// 2. Queries using `db.info()` to fetch the current database name
    /// 3. Sanitizes the connection string to remove password information
    /// 4. Returns the collected information in a `Neo4jConnectionInfo` struct
    ///
    /// If any query fails, appropriate defaults are used and warnings are logged.
    ///
    /// # Errors
    ///
    /// Returns an error wrapped in `InstrumentationResult` if critical issues arise
    /// during Neo4j interaction.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let connection_info = get_connection_info(&graph, "bolt://localhost:7687").await?;
    /// println!(
    ///     "Connected to Neo4j database: {}, version: {} at {}",
    ///     connection_info.database_name,
    ///     connection_info.version,
    ///     connection_info.connection_string
    /// );
    /// ```
    async fn get_connection_info(
        graph: &Graph,
        uri: &str,
    ) -> InstrumentationResult<Neo4jConnectionInfo> {
        // First, try to get the database name and version from system info
        let info_query = neo4rs::query(
            "
            CALL dbms.components() YIELD name, versions, edition
            WITH name, versions[0] as version, edition
            WHERE name = 'Neo4j Kernel'
            RETURN version
        ",
        );

        let mut version = "unknown".to_string();
        if let Ok(mut result) = graph.execute(info_query).await {
            if let Ok(Some(row)) = result.next().await {
                version = row.get("version").unwrap_or_else(|_| "unknown".to_string());
            }
        } else {
            warn!("Failed to retrieve Neo4j version information, using 'unknown'");
        }

        // Try to get current database name
        let db_query = neo4rs::query("CALL db.info() YIELD name RETURN name");
        let mut database_name = "neo4j".to_string();
        if let Ok(mut result) = graph.execute(db_query).await {
            if let Ok(Some(row)) = result.next().await {
                database_name = row.get("name").unwrap_or_else(|_| "neo4j".to_string());
            }
        } else {
            warn!("Failed to retrieve current database name, using 'neo4j'");
        }

        // Sanitize the connection string to remove sensitive information
        let connection_string = Self::sanitize_connection_string(uri);

        // Parse the connection URI to extract server address and port
        let (server_address, server_port) = Self::parse_connection_uri(uri);

        Ok(Neo4jConnectionInfo {
            database_name,
            version,
            connection_string,
            server_address,
            server_port,
        })
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
    fn test_connection_info() {
        let info = Neo4jConnectionInfo {
            database_name: "test".to_string(),
            version: "5.0.0".to_string(),
            connection_string: "bolt://localhost:7687".to_string(),
            server_address: "localhost".to_string(),
            server_port: Some(7687),
        };

        assert_eq!(info.database_name, "test");
        assert_eq!(info.version, "5.0.0");
        assert_eq!(info.connection_string, "bolt://localhost:7687");
        assert_eq!(info.server_address, "localhost");
        assert_eq!(info.server_port, Some(7687));
    }

    #[test]
    fn test_sanitize_connection_string() {
        // Test with password
        assert_eq!(
            InstrumentedGraph::sanitize_connection_string("bolt://user:password@localhost:7687"),
            "bolt://user:****@localhost:7687"
        );

        // Test without password
        assert_eq!(
            InstrumentedGraph::sanitize_connection_string("bolt://localhost:7687"),
            "bolt://localhost:7687"
        );

        // Test with neo4j+s protocol
        assert_eq!(
            InstrumentedGraph::sanitize_connection_string("neo4j+s://user:secret@host.com:7687"),
            "neo4j+s://user:****@host.com:7687"
        );
    }

    #[test]
    fn test_parse_connection_uri() {
        // Test basic bolt URI
        let (addr, port) = InstrumentedGraph::parse_connection_uri("bolt://localhost:7687");
        assert_eq!(addr, "localhost");
        assert_eq!(port, Some(7687));

        // Test with credentials
        let (addr, port) =
            InstrumentedGraph::parse_connection_uri("bolt://user:pass@server.com:7687");
        assert_eq!(addr, "server.com");
        assert_eq!(port, Some(7687));

        // Test neo4j+s protocol
        let (addr, port) = InstrumentedGraph::parse_connection_uri("neo4j+s://db.example.com:7473");
        assert_eq!(addr, "db.example.com");
        assert_eq!(port, Some(7473));

        // Test without port
        let (addr, port) = InstrumentedGraph::parse_connection_uri("bolt://localhost");
        assert_eq!(addr, "localhost");
        assert_eq!(port, None);

        // Test with IP address
        let (addr, port) = InstrumentedGraph::parse_connection_uri("bolt://192.168.1.1:7687");
        assert_eq!(addr, "192.168.1.1");
        assert_eq!(port, Some(7687));

        // Test with IPv6 address (brackets should be preserved)
        let (addr, port) = InstrumentedGraph::parse_connection_uri("bolt://[::1]:7687");
        assert_eq!(addr, "[::1]");
        assert_eq!(port, Some(7687));
    }
}
