# otel-instrumentation-neo4jrs

[![Crates.io](https://img.shields.io/crates/v/otel-instrumentation-neo4jrs.svg)](https://crates.io/crates/otel-instrumentation-neo4jrs)
[![Documentation](https://docs.rs/otel-instrumentation-neo4jrs/badge.svg)](https://docs.rs/otel-instrumentation-neo4jrs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Maintenance](https://img.shields.io/badge/maintenance-experimental-blue.svg)]()

OpenTelemetry instrumentation for the [neo4rs](https://crates.io/crates/neo4rs) Neo4j driver, providing basic tracing for Neo4j database operations.

## Features

- Basic OpenTelemetry tracing for Neo4j operations (`execute`, `run`, `start_txn`)
- Optional metrics collection (query durations, transaction counts, connection tracking)
- Drop-in replacement wrapper for `neo4rs::Graph`
- Extension traits for adding tracing to existing connections
- Connection metadata collection (database name, server info, version)

## Limitations

Due to the neo4rs API design, this instrumentation has several limitations:

- **No query text recording** - neo4rs doesn't expose query text from `Query` objects
- **No operation type detection** - Cannot extract operation types (MATCH, CREATE, etc.) from queries
- **No parameter access** - Query parameters are not accessible for instrumentation
- **Basic span names only** - Span names default to function names (`execute`, `run`, etc.)
- **Limited query modification** - Cannot add comments or modify queries for better tracing

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
otel-instrumentation-neo4jrs = "0.1"
neo4rs = "0.8"
opentelemetry = "0.30"
tracing = "0.1"

# For metrics support
otel-instrumentation-neo4jrs = { version = "0.1", features = ["metrics"] }
```

## Usage

### Basic Usage

```rust
use otel_instrumentation_neo4jrs::InstrumentedGraph;
use neo4rs::{ConfigBuilder, query};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    let config = ConfigBuilder::default()
        .uri("bolt://localhost:7687")
        .user("neo4j")
        .password("password")
        .build()?;
    
    let graph = InstrumentedGraph::connect(config).await?;
    
    // Execute queries - basic tracing is automatic
    graph.execute(query("CREATE (n:Person {name: 'Alice'})")).await?;
    graph.run(query("MATCH (n:Person) RETURN count(n)")).await?;
    
    Ok(())
}
```

### With Extension Traits

```rust
use neo4rs::Graph;
use otel_instrumentation_neo4jrs::GraphExt;

let graph = Graph::new("bolt://localhost:7687", "neo4j", "password").await?;

// Add basic tracing
let instrumented = graph.with_telemetry().await?;

// Or trace individual queries
graph.execute_traced(query("MATCH (n) RETURN count(n)")).await?;
```

### With Metrics (Optional)

```rust,ignore
use otel_instrumentation_neo4jrs::InstrumentedGraphBuilder;
use opentelemetry_sdk::metrics::SdkMeterProvider;

let meter_provider = SdkMeterProvider::builder().build();
let meter = meter_provider.meter("neo4j");

let graph = InstrumentedGraphBuilder::new(config)
    .with_metrics(meter)
    .build()
    .await?;
```

### Transaction Support

```rust
let txn = graph.start_txn().await?;  // Creates a transaction span

// Operations within transaction create child spans
txn.run(query("CREATE (n:Order {id: 1})")).await?;
txn.commit().await?;  // Records completion
```

## Span Attributes

Spans include basic OpenTelemetry semantic convention attributes:

- `db.system` = "neo4j"
- `otel.kind` = "client"
- `db.name` - Database name (retrieved from server)
- `server.address` - Server address (from `NEO4J_SERVER_ADDRESS` env var, defaults to "localhost")
- `server.port` - Server port (from `NEO4J_SERVER_PORT` env var, defaults to 7687)
- `db.version` - Neo4j server version (queried from server)

**Note**: Due to neo4rs limitations, query text, operation types, and parameters are not available as span attributes.

## Metrics (with `metrics` feature)

When metrics are enabled:

| Metric | Type | Description |
|--------|------|-------------|
| `neo4j.queries.total` | Counter | Total queries executed |
| `neo4j.query.duration` | Histogram | Query execution time (ms) |
| `neo4j.transactions.total` | Counter | Transactions started |
| `neo4j.transaction.duration` | Histogram | Transaction duration (ms) |
| `neo4j.connections.active` | UpDownCounter | Active connections |
| `neo4j.errors.total` | Counter | Total errors |

## Environment Variables

- `NEO4J_SERVER_ADDRESS` - Server address for telemetry (default: "localhost")
- `NEO4J_SERVER_PORT` - Server port for telemetry (default: 7687)
- `SERVICE_NAME` - Service name for spans

## What This Library Actually Does

This library provides:

1. **Wrapper types** that implement the same interface as neo4rs types
2. **Basic span creation** for database operations with OpenTelemetry semantic conventions
3. **Connection metadata collection** by querying the Neo4j server
4. **Optional metrics** for operation timing and counting
5. **Extension traits** for easy integration with existing code

This library does NOT provide:

1. Query text extraction or recording
2. Automatic operation type detection (MATCH, CREATE, etc.)
3. Parameter value access or logging  
4. Query modification or comment injection
5. Advanced query analysis or optimization insights

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Related Projects

- [neo4rs](https://github.com/neo4j-labs/neo4rs) - The Neo4j driver being instrumented
- [OpenTelemetry Rust](https://github.com/open-telemetry/opentelemetry-rust) - OpenTelemetry implementation for Rust
- [tracing](https://github.com/tokio-rs/tracing) - Application-level tracing for Rust

## Support
- Documentation: [docs.rs](https://docs.rs/otel-instrumentation-neo4jrs)
