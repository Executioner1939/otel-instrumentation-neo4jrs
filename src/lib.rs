/*!
`otel-instrumentation-neo4jrs` provides a simple wrapper around neo4rs connections that adds
OpenTelemetry tracing instrumentation using the `tracing` crate's `#[instrument]` macro.

# Usage

This crate provides a thin wrapper that follows the standard Rust pattern of wrapping
a type and delegating methods while adding instrumentation.

# Example

```rust,no_run
use otel_instrumentation_neo4jrs::InstrumentedGraph;
use neo4rs::query;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing (you need to set up your tracing subscriber)
    tracing_subscriber::fmt::init();

    // Connect using the instrumented wrapper
    let graph = InstrumentedGraph::connect(
        "bolt://localhost:7687",
        "neo4j",
        "password"
    ).await?;

    // Or wrap an existing Graph
    // let graph = neo4rs::Graph::new(...).await?;
    // let graph = InstrumentedGraph::new(graph);

    // Use it exactly like neo4rs::Graph - all methods are instrumented
    graph.run(query("CREATE (n:Person {name: 'Alice'})")).await?;

    // Transactions are also instrumented
    let mut txn = graph.start_txn().await?;
    txn.run(query("CREATE (n:Order {id: 1})")).await?;
    txn.commit().await?;

    Ok(())
}
```

# Features

- **Simple wrapper pattern** - Just wraps `neo4rs::Graph` and adds tracing
- **Drop-in replacement** - Implements `Deref` and `AsRef<Graph>` for compatibility
- **Automatic instrumentation** - All methods use `#[instrument]` with OpenTelemetry semantic conventions
- **Zero overhead when disabled** - Tracing macros compile to no-ops when not enabled

# Limitations

Due to neo4rs API limitations, query text and parameters cannot be extracted for tracing.
Span names default to the method being called (execute, run, `start_txn`, etc.).

*/
#![warn(clippy::all, clippy::pedantic)]

pub mod graph;
pub mod metrics;
pub mod txn;

pub use graph::InstrumentedGraph;
pub use metrics::{MetricsBuilder, Neo4jMetrics};
pub use txn::InstrumentedTxn;
