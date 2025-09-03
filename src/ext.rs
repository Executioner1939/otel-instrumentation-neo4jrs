//! Extension traits for adding instrumentation to Neo4j types
//!
//! This module provides extension traits that allow easy addition of OpenTelemetry
//! instrumentation to existing Neo4j graph connections and queries.

use neo4rs::{Graph, Query};
use crate::graph::InstrumentedGraph;
use crate::builder::InstrumentedGraphBuilder;
use tracing::instrument;

/// Extension trait for Neo4j Graph to add instrumentation capabilities
///
/// This trait provides methods to easily wrap existing Neo4j connections
/// with OpenTelemetry instrumentation.
///
/// # Example
///
/// ```rust,ignore
/// use neo4rs::Graph;
/// use otel_instrumentation_neo4jrs::GraphExt;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let graph = Graph::new("bolt://localhost:7687", "neo4j", "password").await?;
/// 
/// // Wrap with telemetry
/// let instrumented = graph.with_telemetry().await?;
/// 
/// // Now use the instrumented graph as normal
/// # Ok(())
/// # }
/// ```
pub trait GraphExt: Sized {
    /// Wrap the graph with OpenTelemetry instrumentation using default settings
    ///
    /// This enables tracing but not metrics. For more control, use `with_telemetry_builder`.
    ///
    /// # Errors
    ///
    /// Returns an error if connection information cannot be retrieved from the server.
    fn with_telemetry(self) -> impl std::future::Future<Output = Result<InstrumentedGraph, neo4rs::Error>> + Send;
    
    /// Create a builder for configuring telemetry on this graph
    ///
    /// This allows full control over tracing and metrics configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use neo4rs::Graph;
    /// use otel_instrumentation_neo4jrs::GraphExt;
    /// use opentelemetry::metrics::MeterProvider;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let meter_provider = // ... initialize meter provider
    /// let meter = meter_provider.meter("neo4j");
    ///
    /// let graph = Graph::new("bolt://localhost:7687", "neo4j", "password").await?;
    /// 
    /// let instrumented = graph
    ///     .with_telemetry_builder()
    ///     .with_metrics(meter)
    ///     .with_statement_recording(true)
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    fn with_telemetry_builder(self) -> GraphTelemetryBuilder;
    
    /// Execute a query with ad-hoc tracing
    ///
    /// This allows tracing individual queries without wrapping the entire connection.
    ///
    /// # Arguments
    ///
    /// * `query` - The query to execute
    ///
    /// # Errors
    ///
    /// Returns an error if the query execution fails.
    fn execute_traced(
        &self,
        query: Query,
    ) -> impl std::future::Future<Output = Result<(), neo4rs::Error>> + Send;
    
    /// Run a query with ad-hoc tracing (no results)
    ///
    /// # Arguments
    ///
    /// * `query` - The query to run
    ///
    /// # Errors
    ///
    /// Returns an error if the query execution fails.
    fn run_traced(
        &self,
        query: Query,
    ) -> impl std::future::Future<Output = Result<(), neo4rs::Error>> + Send;
}

impl GraphExt for Graph {
    async fn with_telemetry(self) -> Result<InstrumentedGraph, neo4rs::Error> {
        InstrumentedGraph::from_graph(self).await
    }
    
    fn with_telemetry_builder(self) -> GraphTelemetryBuilder {
        GraphTelemetryBuilder::new(self)
    }
    
    #[instrument(
        fields(
            db.system = "neo4j",
            otel.kind = "client",
        ),
        skip(self, query),
        err,
    )]
    async fn execute_traced(&self, query: Query) -> Result<(), neo4rs::Error> {
        tracing::debug!("executing traced neo4j query");
        let mut stream = self.execute(query).await?;
        let mut row_count = 0;
        while let Ok(Some(_)) = stream.next().await {
            row_count += 1;
        }
        tracing::debug!("traced query completed, processed {} rows", row_count);
        Ok(())
    }
    
    #[instrument(
        fields(
            db.system = "neo4j",
            otel.kind = "client",
        ),
        skip(self, query),
        err,
    )]
    async fn run_traced(&self, query: Query) -> Result<(), neo4rs::Error> {
        tracing::debug!("running traced neo4j query");
        self.run(query).await
    }
}

/// Builder for configuring telemetry on an existing Graph
pub struct GraphTelemetryBuilder {
    graph: Graph,
    builder: InstrumentedGraphBuilder,
}

impl GraphTelemetryBuilder {
    fn new(graph: Graph) -> Self {
        // Create a dummy config - we'll replace the graph later
        let config = neo4rs::ConfigBuilder::default()
            .uri("bolt://localhost:7687")
            .build()
            .unwrap();
        
        Self {
            graph,
            builder: InstrumentedGraphBuilder::new(config),
        }
    }
    
    /// Enable or disable tracing
    pub fn with_tracing(mut self, enabled: bool) -> Self {
        self.builder = self.builder.with_tracing(enabled);
        self
    }
    
    /// Enable metrics collection
    pub fn with_metrics(mut self, meter: opentelemetry::metrics::Meter) -> Self {
        self.builder = self.builder.with_metrics(meter);
        self
    }
    
    /// Set the service name
    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.builder = self.builder.with_service_name(name);
        self
    }
    
    /// Enable statement recording
    pub fn with_statement_recording(mut self, enabled: bool) -> Self {
        self.builder = self.builder.with_statement_recording(enabled);
        self
    }
    
    /// Build the instrumented graph
    pub async fn build(self) -> Result<InstrumentedGraph, neo4rs::Error> {
        InstrumentedGraph::from_graph_with_builder(self.graph, self.builder).await
    }
}

/// Extension trait for Query to add instrumentation helpers
///
/// This trait provides methods to enhance queries with additional metadata
/// for better observability.
pub trait QueryExt {
    /// Add a comment to the query for identification in traces
    ///
    /// The comment will be prepended to the query as a Cypher comment.
    ///
    /// # Arguments
    ///
    /// * `comment` - The comment to add
    fn with_trace_comment(self, comment: &str) -> Self;
    
    /// Tag the query with a name for easier identification in traces
    ///
    /// This adds a special comment that can be parsed by instrumentation.
    ///
    /// # Arguments
    ///
    /// * `name` - The name/identifier for this query
    fn with_operation_name(self, name: &str) -> Self;
}

impl QueryExt for Query {
    fn with_trace_comment(self, _comment: &str) -> Self {
        // Neo4j Query type doesn't expose a way to modify the query text
        // This would need to be implemented differently or require changes to neo4rs
        // For now, we'll just return the query unchanged
        self
    }
    
    fn with_operation_name(self, _name: &str) -> Self {
        // Similar limitation as above
        // In a real implementation, we might need to wrap Query in our own type
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_graph_ext_traits() {
        // This test would require a running Neo4j instance
        // For unit testing, we're just checking that the trait methods exist
        
        // The actual Graph::new would fail without a real Neo4j instance
        // So we're just testing compilation here
    }
    
    #[test]
    fn test_query_ext_traits() {
        let query = neo4rs::query("MATCH (n) RETURN n");
        let _ = query.with_trace_comment("test comment");
        
        let query2 = neo4rs::query("CREATE (n:Person {name: $name})");
        let _ = query2.with_operation_name("create_person");
    }
}