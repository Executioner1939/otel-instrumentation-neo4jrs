/*!
`otel-instrumentation-neo4jrs` provides connection structures that can be used as drop in
replacements for neo4rs connections with extra OpenTelemetry tracing and logging.

# Usage

## Limitations

Due to neo4rs API limitations, this instrumentation provides basic telemetry without query text extraction:
- Query text is not available as neo4rs Query type doesn't expose it
- Operation names cannot be extracted from queries
- Stream instrumentation is limited due to transaction handle requirements

# Establishing a connection

`otel-instrumentation-neo4jrs` provides an instrumented Graph wrapper that 
implements the same interface as the `neo4rs::Graph`. As this struct provides the same
API as the underlying `neo4rs` implementation, establishing a connection is done in the 
same way as the original crate.

```
use otel_instrumentation_neo4jrs::InstrumentedGraph;
use neo4rs::ConfigBuilder;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let config = ConfigBuilder::default()
    .uri("bolt://localhost:7687")
    .user("neo4j")
    .password("password")
    .build()
    .unwrap();

let graph = InstrumentedGraph::connect(config).await?;
# Ok(())
# }
```

This connection can then be used with neo4rs methods such as
`execute`, `run`, and `start_txn`.

# Code reuse

In some applications it may be desirable to be able to use both instrumented and
uninstrumented connections. For example, in the tests for a library. To achieve this
you can use traits or generic parameters in your functions.

```
async fn use_connection<G>(graph: &G) -> Result<(), Box<dyn std::error::Error>>
where
    G: AsRef<neo4rs::Graph>,
{
    // Your graph operations here
    Ok(())
}
```

This function will accept both `neo4rs::Graph` and `InstrumentedGraph`.

# OpenTelemetry Semantic Conventions

This crate follows the [OpenTelemetry semantic conventions for database operations](https://opentelemetry.io/docs/specs/semconv/database/).

## Span Attributes

All spans include the following attributes following OpenTelemetry semantic conventions:

### Required Attributes
- `db.system`: Always set to "neo4j"
- `otel.kind`: Set to "client"

### Connection Attributes (when available)
- `db.name`: The name of the Neo4j database
- `server.address`: Neo4j server hostname or IP address  
- `server.port`: Neo4j server port number
- `db.version`: Neo4j server version string

### Operation Attributes (when applicable)
- `db.operation.name`: The operation type (e.g., "MATCH", "CREATE", "MERGE")
- `db.collection.name`: Node labels or relationship types being accessed
- `db.query.summary`: A low-cardinality summary of the query

### Unavailable Attributes (due to API limitations)
- `db.query.text`: Not available as neo4rs doesn't expose query text
- `db.query.parameter.<key>`: Not available as neo4rs doesn't expose query parameters
- `db.operation.name`: Cannot be extracted from neo4rs Query type

### Error Attributes (when errors occur)
- `error.type`: The error classification
- `db.response.status_code`: Neo4j-specific error codes when available

## Span Names

Due to API limitations, span names default to the function being called:
- `execute` for query execution
- `run` for fire-and-forget queries
- `start_txn` for transaction creation
- `commit` for transaction commits
- `rollback` for transaction rollbacks

## Security Considerations

### Sensitive Information

This instrumentation does not record query text or parameters due to neo4rs API limitations,
which provides good security by default. Connection strings are never recorded in spans
as they may contain passwords.

### Best Practices

- Connection information is gathered from Neo4j system queries, not connection strings
- Only basic connection metadata (database name, server address, version) is recorded
- Consider using OpenTelemetry sampling to reduce data volume in production

## Notes

### Async Support

This instrumentation is built for async Rust and requires a tokio runtime.
All instrumented operations return the same async types as the underlying neo4rs driver.

### Performance Impact

The instrumentation adds minimal overhead:
- Span creation and attribute setting
- Timestamp recording for operation duration  
- Connection metadata queries (cached after first retrieval)

### Transaction Support

Transactions are fully supported with proper span hierarchies:
- Transaction creation creates a span
- Operations within transactions create child spans
- Transaction commit/rollback creates completion spans

## TODO

- [ ] Add support for batch operations
- [ ] Add metrics collection for connection pool statistics
- [ ] Add support for custom span processors
- [ ] Consider query sanitization utilities

*/
#![warn(clippy::all, clippy::pedantic)]

pub mod graph;
pub mod txn;
pub mod error;
pub mod metrics;
pub mod builder;
pub mod ext;

pub use graph::InstrumentedGraph;
pub use txn::InstrumentedTxn;
pub use error::InstrumentationError;
pub use metrics::{Neo4jMetrics, MetricsBuilder};
pub use builder::{InstrumentedGraphBuilder, TelemetryConfig};
pub use ext::{GraphExt, QueryExt};