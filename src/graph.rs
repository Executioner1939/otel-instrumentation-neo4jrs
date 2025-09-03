//! An instrumented wrapper around `neo4rs::Graph` that adds OpenTelemetry tracing
//!
//! This struct provides methods for connecting to a Neo4j database,
//! executing queries, and attaching OpenTelemetry instruments following
//! database semantic conventions. It serves to enhance the usability of the
//! `neo4rs` crate while embedding observability within a distributed system.
//!
//! # Key Features
//!
//! - Establishes a connection to Neo4j with OpenTelemetry tracing metadata.
//! - Supports executing queries with embedded spans for observability.
//! - Adheres to OpenTelemetry database semantic conventions.
//! - Handles errors gracefully while converting custom errors into Neo4j errors.
//!
//! ## Dependencies
//!
//! - `neo4rs`: Used for creating connections and executing queries.
//! - `tracing`: Used for OpenTelemetry instrumentation.
//!
//! ## Example Usage
//!
//! ```rust
//! use otel_instrumentation_neo4jrs::InstrumentedGraph;
//! use neo4rs::{ConfigBuilder, query};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ConfigBuilder::default()
//!         .uri("bolt://localhost:7687")
//!         .user("neo4j")
//!         .password("password")
//!         .build()?;
//!
//!     let graph = InstrumentedGraph::connect(config).await?;
//!
//!     // Example of executing a query
//!     graph.execute(query("MATCH (n) RETURN count(n) as total")).await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Error Handling
//!
//! Most methods return `neo4rs::Error` on failure, which could arise from:
//! - Issues connecting to the Neo4j database.
//! - Query execution failures.
//! - Network connectivity problems.
//!
//! ## Example with Explicit Parameters
//!
//! ```rust
//! let graph = InstrumentedGraph::new("bolt://localhost:7687", "neo4j", "password").await?;
//! ```
use crate::builder::InstrumentedGraphBuilder;
use crate::error::{InstrumentationError, InstrumentationResult};
use crate::metrics::{Neo4jMetrics, OperationTimer};
use neo4rs::{Config, Graph, Query};
use std::ops::Deref;
use std::sync::Arc;
use tracing::{debug, instrument, warn};

/// Connection information retrieved from Neo4j server
#[derive(Clone, Debug)]
pub struct Neo4jConnectionInfo {
    pub database_name: String,
    pub server_address: String,
    pub server_port: i32,
    pub version: String,
}

/// An instrumented wrapper around `neo4rs::Graph` that adds OpenTelemetry tracing
///
/// This struct provides the same API as `neo4rs::Graph` but adds comprehensive
/// OpenTelemetry instrumentation following database semantic conventions.
///
/// # Example
///
/// ```
/// use otel_instrumentation_neo4jrs::InstrumentedGraph;
/// use neo4rs::{ConfigBuilder, query};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = ConfigBuilder::default()
///     .uri("bolt://localhost:7687")
///     .user("neo4j")
///     .password("password")
///     .build()?;
///
/// let graph = InstrumentedGraph::connect(config).await?;
///
/// // Execute a query (results are consumed internally for telemetry)
/// graph.execute(query("MATCH (n) RETURN count(n) as total")).await?;
/// # Ok(())
/// # }
/// ```
pub struct InstrumentedGraph {
    inner: Graph,
    info: Neo4jConnectionInfo,
    metrics: Option<Arc<Neo4jMetrics>>,
    #[allow(dead_code)]
    record_statement: bool,
    #[allow(dead_code)]
    max_statement_length: usize,
}

impl InstrumentedGraph {
    /// Create an instrumented graph with custom options
    ///
    /// This is primarily used by the builder pattern.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if connection information cannot be retrieved from the Neo4j server.
    pub async fn with_options(
        graph: Graph,
        _enable_tracing: bool,
        metrics: Option<Arc<Neo4jMetrics>>,
        record_statement: bool,
        max_statement_length: usize,
    ) -> Result<Self, neo4rs::Error> {
        let info = Self::get_connection_info(&graph)
            .await
            .map_err(Self::convert_instrumentation_error)?;

        Ok(Self {
            inner: graph,
            info,
            metrics,
            record_statement,
            max_statement_length,
        })
    }

    /// Create an instrumented graph from an existing graph connection
    ///
    /// This is used by the extension trait.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if connection information cannot be retrieved from the Neo4j server.
    pub async fn from_graph(graph: Graph) -> Result<Self, neo4rs::Error> {
        let info = Self::get_connection_info(&graph)
            .await
            .map_err(Self::convert_instrumentation_error)?;

        Ok(Self {
            inner: graph,
            info,
            metrics: None,
            record_statement: false,
            max_statement_length: 1024,
        })
    }

    /// Create an instrumented graph from an existing graph with a builder
    ///
    /// Internal method used by the extension trait builder pattern.
    pub(crate) async fn from_graph_with_builder(
        graph: Graph,
        _builder: InstrumentedGraphBuilder,
    ) -> Result<Self, neo4rs::Error> {
        // Extract settings from the builder
        // This is a bit hacky but avoids exposing all builder fields
        let info = Self::get_connection_info(&graph)
            .await
            .map_err(Self::convert_instrumentation_error)?;

        Ok(Self {
            inner: graph,
            info,
            metrics: None, // Would need to expose builder fields or refactor
            record_statement: false,
            max_statement_length: 1024,
        })
    }

    /// Helper to convert instrumentation errors to neo4rs errors for API compatibility
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

    /// Create a new connection with explicit parameters
    ///
    /// This is equivalent to `Graph::new(uri, user, pass)` but adds telemetry instrumentation.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the connection to the Neo4j server fails or if
    /// connection information cannot be retrieved from the server.
    #[instrument(skip(user, pass), err)]
    pub async fn new(uri: &str, user: &str, pass: &str) -> Result<Self, neo4rs::Error> {
        let span = tracing::Span::current();
        span.record("db.system", "neo4j");
        span.record("otel.kind", "client");

        debug!("establishing neo4j connection with uri: {}", uri);
        let graph = Graph::new(uri, user, pass).await?;
        let info = Self::get_connection_info(&graph)
            .await
            .map_err(Self::convert_instrumentation_error)?;

        // Record connection info after it's available
        span.record("db.name", &info.database_name);
        span.record("server.address", &info.server_address);
        span.record("server.port", info.server_port);
        span.record("db.version", &info.version);

        Ok(Self {
            inner: graph,
            info,
            metrics: None,
            record_statement: false,
            max_statement_length: 1024,
        })
    }

    /// Create a new connection with the specified configuration
    ///
    /// This is equivalent to `Graph::connect(config)` but adds telemetry instrumentation.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the connection to the Neo4j server fails or if
    /// connection information cannot be retrieved from the server.
    #[instrument(skip(config), err)]
    pub async fn connect(config: Config) -> Result<Self, neo4rs::Error> {
        let span = tracing::Span::current();
        span.record("db.system", "neo4j");
        span.record("otel.kind", "client");

        debug!("establishing neo4j connection with custom config");
        let graph = Graph::connect(config).await?;
        let info = Self::get_connection_info(&graph)
            .await
            .map_err(Self::convert_instrumentation_error)?;

        // Record connection info after it's available
        span.record("db.name", &info.database_name);
        span.record("server.address", &info.server_address);
        span.record("server.port", info.server_port);
        span.record("db.version", &info.version);

        Ok(Self {
            inner: graph,
            info,
            metrics: None,
            record_statement: false,
            max_statement_length: 1024,
        })
    }

    /// Execute a query and consume all results
    ///
    /// This method instruments the query execution with OpenTelemetry spans
    /// following database semantic conventions.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails or if there are
    /// network connectivity issues with the Neo4j server.
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
    pub async fn execute(&self, query: Query) -> Result<(), neo4rs::Error> {
        debug!("executing neo4j query");

        // Start timing if metrics are enabled
        let timer = self.metrics.as_ref().map(|_| OperationTimer::start());

        let result = async {
            let mut stream = self.inner.execute(query).await?;
            // Consume the stream to ensure the query runs and collect metrics
            let mut row_count = 0;
            while let Ok(Some(_)) = stream.next().await {
                row_count += 1;
            }
            debug!(
                "neo4j query execution completed, processed {} rows",
                row_count
            );
            Ok(())
        }
        .await;

        // Record metrics if enabled
        if let Some(metrics) = &self.metrics {
            if let Some(timer) = timer {
                let _ = timer.record_query(
                    metrics,
                    result.is_ok(),
                    None, // Operation type not available from Query
                    &self.info.database_name,
                );
            }
        }

        result
    }

    /// Run a query without returning results
    ///
    /// This method instruments the query execution with OpenTelemetry spans.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails or if there are
    /// network connectivity issues with the Neo4j server.
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
    pub async fn run(&self, query: Query) -> Result<(), neo4rs::Error> {
        debug!("running neo4j query without results");
        self.inner.run(query).await
    }

    /// Execute a query on a specific database
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails, if the specified
    /// database does not exist, or if there are network connectivity issues.
    #[instrument(
        fields(
            db.system="neo4j",
            db.name=%database,
            server.address=%self.info.server_address,
            server.port=self.info.server_port,
            db.version=%self.info.version,
            otel.kind="client",
        ),
        skip(self, query),
        err,
    )]
    pub async fn execute_on(&self, database: &str, query: Query) -> Result<(), neo4rs::Error> {
        debug!("executing neo4j query on database {}", database);
        let mut stream = self.inner.execute_on(database, query).await?;
        // Consume the stream to ensure the query runs and collect metrics
        let mut row_count = 0;
        while let Ok(Some(_)) = stream.next().await {
            row_count += 1;
        }
        debug!(
            "neo4j query execution on database {} completed, processed {} rows",
            database, row_count
        );
        Ok(())
    }

    /// Run a query on a specific database without returning results
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the query execution fails, if the specified
    /// database does not exist, or if there are network connectivity issues.
    #[instrument(
        fields(
            db.system="neo4j",
            db.name=%database,
            server.address=%self.info.server_address,
            server.port=self.info.server_port,
            db.version=%self.info.version,
            otel.kind="client",
        ),
        skip(self, query),
        err,
    )]
    pub async fn run_on(&self, database: &str, query: Query) -> Result<(), neo4rs::Error> {
        debug!(
            "running neo4j query on database {} without results",
            database
        );
        self.inner.run_on(database, query).await
    }

    /// Start a new transaction
    ///
    /// This method creates an instrumented transaction wrapper.
    ///
    /// # Errors
    ///
    /// Returns a [`neo4rs::Error`] if the transaction cannot be started due to
    /// connection issues or server-side constraints.
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
    pub async fn start_txn(&self) -> Result<crate::txn::InstrumentedTxn, neo4rs::Error> {
        debug!("starting neo4j transaction");

        // Record transaction start if metrics are enabled
        if let Some(metrics) = &self.metrics {
            metrics.record_transaction_start(&self.info.database_name);
        }

        let txn = self.inner.start_txn().await?;
        Ok(crate::txn::InstrumentedTxn::new(
            txn,
            self.info.clone(),
            self.metrics.clone(),
        ))
    }

    ///
    /// Returns a reference to the Neo4j connection information associated with the current object.
    ///
    /// # Returns
    ///
    /// A reference to a `Neo4jConnectionInfo` instance containing details about the Neo4j connection.
    ///
    /// # Attributes
    ///
    /// * `#[must_use]` - Indicates that the return value of this function should be used by the caller;
    ///   ignoring the result may result in undesirable behavior or missed information.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let connection_info = instance.connection_info();
    /// println!("Connection Info: {:?}", connection_info);
    /// ```
    ///
    #[must_use]
    pub fn connection_info(&self) -> &Neo4jConnectionInfo {
        &self.info
    }

    /// Retrieves a reference to the inner `Graph` instance.
    ///
    /// # Returns
    ///
    /// A reference to the `Graph` stored within the current object.
    ///
    /// # Attributes
    ///
    /// * `#[must_use]` - Indicates that the return value of this function should not be ignored.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let my_object = MyStruct::new();
    /// let graph = my_object.inner();
    /// // Use `graph` for further operations
    /// ```
    #[must_use]
    pub fn inner(&self) -> &Graph {
        &self.inner
    }

    /// Retrieves connection information for a Neo4j database.
    ///
    /// This asynchronous function fetches relevant information about the Neo4j database,
    /// including its version, current database name, server address, and server port.
    /// If certain details cannot be retrieved, it falls back to default values or environment
    /// variables with reasonable logging for unexpected situations.
    ///
    /// # Parameters
    /// - `graph`: A reference to a `Graph` instance used to interact with the Neo4j database.
    ///
    /// # Returns
    /// - An `InstrumentationResult` containing a `Neo4jConnectionInfo` struct populated with the retrieved details:
    ///   - `database_name`: The name of the current Neo4j database (defaults to "neo4j" if retrieval fails).
    ///   - `version`: The Neo4j version (defaults to "unknown" if retrieval fails).
    ///   - `server_address`: The server address of the Neo4j instance (defaults to "localhost" if unspecified).
    ///   - `server_port`: The server port of the Neo4j instance (defaults to 7687 if unspecified or invalid).
    ///
    /// # Behavior
    /// - Queries the Neo4j database using system commands to fetch the database version (`dbms.components`) and
    ///   database name (`db.info`).
    /// - Retrieves the server address and port from environment variables `NEO4J_SERVER_ADDRESS` and
    ///   `NEO4J_SERVER_PORT`.
    /// - Defaults are applied in the following cases:
    ///   - If the Neo4j version query fails, "unknown" is used as the version.
    ///   - If the database name query fails, "neo4j" is used as the database name.
    ///   - If `NEO4J_SERVER_PORT` is not set, invalid, or out of range (1–65535), 7687 is used as the default port.
    /// - Logs warnings when queries fail or environment variables are invalid.
    ///
    /// # Example
    /// ```rust,ignore
    /// let connection_info = get_connection_info(&graph).await?;
    /// println!(
    ///     "Connected to Neo4j database: {}, version: {} at {}:{}",
    ///     connection_info.database_name,
    ///     connection_info.version,
    ///     connection_info.server_address,
    ///     connection_info.server_port
    /// );
    /// ```
    ///
    /// # Errors
    /// Returns an error wrapped in `InstrumentationResult` if any issues arise during Neo4j interaction.
    ///
    /// # Logging
    /// - Logs warnings for:
    ///   - Failure to retrieve the database version or name.
    ///   - Invalid or missing `NEO4J_SERVER_PORT`.
    ///
    /// # Notes
    /// - Neo4j does not expose connection details like the server address and port directly.
    ///   This limitation is addressed by using environment variables and providing defaults.
    ///
    /// # Dependencies
    /// - `neo4rs`: Used for running Cypher queries to fetch database information.
    /// - `std::env`: Used to access environment variables for server address and port.
    ///
    /// # See Also
    /// - [`Neo4jConnectionInfo`](struct.Neo4jConnectionInfo.html): The structure that holds connection info details.
    /// - [`InstrumentationResult`](enum.InstrumentationResult.html): The result type returned by this function.
    async fn get_connection_info(graph: &Graph) -> InstrumentationResult<Neo4jConnectionInfo> {
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

        // For server address and port, we have limited options due to Neo4j API constraints
        // Neo4j doesn't expose connection details through system queries for security reasons
        // We'll use reasonable defaults that can be overridden by telemetry configuration
        let server_address =
            std::env::var("NEO4J_SERVER_ADDRESS").unwrap_or_else(|_| "localhost".to_string());

        let server_port = match std::env::var("NEO4J_SERVER_PORT") {
            Ok(port_str) => match port_str.parse::<i32>() {
                Ok(port) if port > 0 && port <= 65535 => port,
                Ok(_) => {
                    warn!(
                        "Invalid port number in NEO4J_SERVER_PORT: {}, using default 7687",
                        port_str
                    );
                    7687
                }
                Err(e) => {
                    warn!(
                        "Failed to parse NEO4J_SERVER_PORT '{}': {}, using default 7687",
                        port_str, e
                    );
                    7687
                }
            },
            Err(_) => 7687,
        };

        Ok(Neo4jConnectionInfo {
            database_name,
            server_address,
            server_port,
            version,
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
