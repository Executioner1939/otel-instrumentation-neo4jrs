# otel-instrumentation-neo4jrs

[![Crates.io](https://img.shields.io/crates/v/otel-instrumentation-neo4jrs.svg)](https://crates.io/crates/otel-instrumentation-neo4jrs)
[![Documentation](https://docs.rs/otel-instrumentation-neo4jrs/badge.svg)](https://docs.rs/otel-instrumentation-neo4jrs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Maintenance](https://img.shields.io/badge/maintenance-experimental-blue.svg)]()

OpenTelemetry instrumentation for the [neo4rs](https://crates.io/crates/neo4rs) Neo4j driver, providing comprehensive tracing and metrics collection for Neo4j graph database operations.

## Features

- 🔍 **Distributed Tracing** - Automatic OpenTelemetry span creation for all Neo4j operations
- 📊 **Metrics Collection** - Track query durations, transaction lifecycles, error rates, and connection statistics
- 🏗️ **Builder Pattern** - Flexible configuration with sensible defaults
- 🔌 **Extension Traits** - Easy integration with existing Neo4j code
- 🎯 **Query Classification** - Automatic operation type detection (MATCH, CREATE, MERGE, etc.)
- 🔒 **Security-First** - Configurable statement recording with privacy controls
- ⚡ **Zero-Cost Abstractions** - Minimal performance overhead when disabled
- 🧩 **Drop-in Replacement** - Compatible with existing neo4rs code

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
otel-instrumentation-neo4jrs = "0.1"
neo4rs = "0.8"
opentelemetry = "0.30"
tracing = "0.1"

# For metrics support (optional)
otel-instrumentation-neo4jrs = { version = "0.1", features = ["metrics"] }

# For all features
otel-instrumentation-neo4jrs = { version = "0.1", features = ["full"] }
```

### Feature Flags

- `metrics` - Enable metrics collection support
- `full` - Enable all features (currently just metrics)

## Quick Start

### Basic Tracing

```rust
use otel_instrumentation_neo4jrs::InstrumentedGraph;
use neo4rs::{ConfigBuilder, query};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing (required)
    tracing_subscriber::fmt::init();
    
    // Create Neo4j configuration
    let config = ConfigBuilder::default()
        .uri("bolt://localhost:7687")
        .user("neo4j")
        .password("password")
        .build()?;
    
    // Create an instrumented connection
    let graph = InstrumentedGraph::connect(config).await?;
    
    // Use as normal - tracing happens automatically
    graph.execute(query("CREATE (n:Person {name: 'Alice'})")).await?;
    
    let mut result = graph.execute(
        query("MATCH (n:Person) RETURN n.name as name")
    ).await?;
    
    while let Some(row) = result.next().await? {
        let name: String = row.get("name")?;
        println!("Found person: {}", name);
    }
    
    Ok(())
}
```

### With Metrics Collection

```rust
use otel_instrumentation_neo4jrs::InstrumentedGraphBuilder;
use neo4rs::ConfigBuilder;
use opentelemetry_sdk::metrics::SdkMeterProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize metrics provider
    let meter_provider = SdkMeterProvider::builder().build();
    let meter = meter_provider.meter("neo4j");
    
    // Create configuration
    let config = ConfigBuilder::default()
        .uri("bolt://localhost:7687")
        .user("neo4j")
        .password("password")
        .build()?;
    
    // Build instrumented connection with metrics
    let graph = InstrumentedGraphBuilder::new(config)
        .with_tracing(true)
        .with_metrics(meter)
        .with_service_name("my-graph-service")
        .with_statement_recording(false)  // Don't record statements for privacy
        .build()
        .await?;
    
    // Metrics are automatically collected for all operations
    graph.execute(query("CREATE (n:Product {name: 'Widget', price: 9.99})")).await?;
    
    Ok(())
}
```

## Usage Patterns

### Extension Traits

Convert existing Neo4j connections to instrumented ones using extension traits:

```rust
use neo4rs::Graph;
use otel_instrumentation_neo4jrs::GraphExt;

// Start with a regular Neo4j connection
let graph = Graph::new("bolt://localhost:7687", "neo4j", "password").await?;

// Add telemetry with default settings (tracing only)
let instrumented = graph.with_telemetry().await?;

// Or use the builder for custom configuration
let instrumented = graph
    .with_telemetry_builder()
    .with_metrics(meter)
    .with_service_name("my-service")
    .with_statement_recording(true)
    .build()
    .await?;
```

### Ad-hoc Tracing

Trace individual operations without wrapping the entire connection:

```rust
use neo4rs::Graph;
use otel_instrumentation_neo4jrs::GraphExt;

let graph = Graph::new("bolt://localhost:7687", "neo4j", "password").await?;

// Trace specific queries only
graph.execute_traced(query("MATCH (n) RETURN count(n) as total")).await?;
graph.run_traced(query("CREATE INDEX IF NOT EXISTS FOR (n:Person) ON (n.email)")).await?;
```

### Transaction Support

Transactions are fully instrumented with proper span hierarchies:

```rust
let mut txn = graph.start_txn().await?;  // Creates transaction span

// Operations within transaction create child spans
txn.run(query("CREATE (n:Order {id: 1, total: 99.99})")).await?;
txn.run(query("CREATE (n:OrderItem {order_id: 1, product: 'Widget'})")).await?;

// Commit or rollback creates completion span
txn.commit().await?;  // Duration and outcome recorded
```

### Code Reuse with Generic Functions

Write functions that work with both instrumented and regular connections:

```rust
async fn count_nodes<G>(graph: &G) -> Result<i64, Box<dyn std::error::Error>>
where
    G: AsRef<neo4rs::Graph>,
{
    let graph = graph.as_ref();
    let mut result = graph.execute(query("MATCH (n) RETURN count(n) as count")).await?;
    
    if let Some(row) = result.next().await? {
        Ok(row.get("count")?)
    } else {
        Ok(0)
    }
}

// Works with both types
let regular_graph = Graph::new(...).await?;
let count1 = count_nodes(&regular_graph).await?;

let instrumented_graph = InstrumentedGraph::connect(...).await?;
let count2 = count_nodes(&instrumented_graph).await?;
```

## Configuration Options

### Builder Configuration

The `InstrumentedGraphBuilder` provides fine-grained control:

| Option | Default | Description |
|--------|---------|-------------|
| `with_tracing` | `true` | Enable/disable OpenTelemetry tracing |
| `with_metrics` | `None` | Provide a Meter to enable metrics collection |
| `with_service_name` | `None` | Set service name for telemetry |
| `with_statement_recording` | `false` | Record Cypher statements in spans (security consideration) |
| `with_max_statement_length` | `1024` | Maximum length for recorded statements |

### Environment Variables

- `SERVICE_NAME` - Default service name if not specified in configuration

## Telemetry Details

### Span Attributes

All spans include OpenTelemetry semantic convention attributes:

**Always Present:**
- `db.system` = "neo4j"
- `otel.kind` = "client"

**Connection Attributes (when available):**
- `db.name` - Database name
- `server.address` - Neo4j server hostname/IP
- `server.port` - Server port number
- `db.version` - Neo4j version string

**Operation Attributes (when detected):**
- `db.operation.name` - Operation type (MATCH, CREATE, etc.)
- `db.collection.name` - Node labels or relationship types
- `db.query.summary` - Low-cardinality query summary

**Error Attributes (on failure):**
- `error.type` - Error classification
- `db.response.status_code` - Neo4j error codes

### Metrics Collected

When metrics are enabled, the following are automatically collected:

| Metric | Type | Description | Labels |
|--------|------|-------------|--------|
| `neo4j.queries.total` | Counter | Total queries executed | `success`, `database`, `operation` |
| `neo4j.query.duration` | Histogram | Query execution time (ms) | `success`, `database`, `operation` |
| `neo4j.transactions.total` | Counter | Transactions started | `database` |
| `neo4j.transaction.duration` | Histogram | Transaction duration (ms) | `database`, `outcome` |
| `neo4j.transaction.commits` | Counter | Successful commits | `database` |
| `neo4j.transaction.rollbacks` | Counter | Transaction rollbacks | `database` |
| `neo4j.errors.total` | Counter | Total errors | `error_type`, `database`, `operation` |
| `neo4j.connections.active` | UpDownCounter | Active connections | - |

## Architecture

This crate follows established patterns for OpenTelemetry instrumentation:

### Design Patterns

1. **Wrapper Pattern** - `InstrumentedGraph` wraps `neo4rs::Graph` while maintaining the same interface
2. **Builder Pattern** - Flexible configuration through `InstrumentedGraphBuilder`
3. **Extension Traits** - `GraphExt` and `QueryExt` for enhancing existing types
4. **Semantic Conventions** - Follows OpenTelemetry database semantic conventions

### Implementation Details

- **Zero-cost when disabled** - Tracing and metrics have minimal overhead when not configured
- **Async-first** - Built for async Rust with tokio runtime
- **Thread-safe** - All types are `Send + Sync` for concurrent use
- **Connection pooling compatible** - Works with connection pools and r2d2

## Performance Considerations

### Overhead

The instrumentation adds minimal overhead:
- ~1-5μs per operation for span creation
- Memory: One span per active operation
- Network: No additional Neo4j roundtrips

### Best Practices

1. **Sampling** - Use OpenTelemetry sampling in production to reduce data volume
2. **Statement Recording** - Disable in production to avoid capturing sensitive data
3. **Metrics Aggregation** - Configure appropriate histogram buckets for your workload
4. **Async Operations** - All instrumentation is async-aware with no blocking calls

## Security Considerations

### Privacy Controls

- **Statement Recording** - Disabled by default to prevent sensitive data exposure
- **Connection Strings** - Never recorded in spans (may contain passwords)  
- **Parameter Values** - Not accessible due to neo4rs API limitations (additional security)
- **Error Messages** - Sanitized to remove potentially sensitive information

### Production Recommendations

1. Keep `with_statement_recording(false)` in production
2. Use OpenTelemetry processors to filter sensitive span attributes
3. Configure appropriate data retention policies
4. Use TLS for exporting telemetry data

## Examples

See the [`examples/`](examples/) directory for complete examples:

- [`metrics_and_tracing.rs`](examples/metrics_and_tracing.rs) - Combined metrics and tracing with various patterns

Run examples with:

```bash
cargo run --example metrics_and_tracing --features metrics
```

## Limitations

Due to neo4rs API constraints:

- Query text and parameters are not directly accessible
- Operation names must be inferred from query patterns
- Stream instrumentation requires transaction handle access
- Query modification (for comments) is not supported

## Contributing

Contributions are welcome! Please see the [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Related Projects

- [neo4rs](https://github.com/neo4j-labs/neo4rs) - The Neo4j driver being instrumented
- [OpenTelemetry Rust](https://github.com/open-telemetry/opentelemetry-rust) - OpenTelemetry implementation for Rust
- [tracing](https://github.com/tokio-rs/tracing) - Application-level tracing for Rust

## Support
- Documentation: [docs.rs](https://docs.rs/otel-instrumentation-neo4jrs)
