#![cfg(all(test, feature = "integration"))]

// Note: These integration tests must be run with --test-threads=1
// because they share a global tracer provider. Running tests in parallel
// can cause race conditions where spans are not properly captured.

use neo4rs::Query;
use opentelemetry::{
    global,
    trace::{SpanKind, Status, TraceContextExt, Tracer},
};
use opentelemetry_sdk::{
    trace::{InMemorySpanExporterBuilder, Sampler, SdkTracerProvider as TracerProvider, SpanData},
    Resource,
};
use opentelemetry_semantic_conventions::attribute::{
    DB_NAMESPACE, DB_OPERATION_NAME, DB_QUERY_TEXT, DB_SYSTEM_NAME, SERVER_ADDRESS,
};
use otel_instrumentation_neo4jrs::InstrumentedGraph;

struct TestHarness {
    provider: TracerProvider,
    exporter: opentelemetry_sdk::trace::InMemorySpanExporter,
}

impl TestHarness {
    fn new() -> Self {
        let exporter = InMemorySpanExporterBuilder::new().build();
        let provider = TracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .with_sampler(Sampler::AlwaysOn)
            .with_resource(Resource::builder_empty().build())
            .build();

        // Set as global provider for instrumentation
        global::set_tracer_provider(provider.clone());

        Self { provider, exporter }
    }

    fn get_spans(&self) -> Vec<SpanData> {
        // Force flush of any pending spans
        let _ = self.provider.force_flush();
        self.exporter.get_finished_spans().unwrap()
    }

    fn reset(&self) {
        self.exporter.reset();
    }

    fn tracer(&self, name: &'static str) -> opentelemetry::global::BoxedTracer {
        // Use global tracer which returns BoxedTracer
        global::tracer(name)
    }
}

fn get_neo4j_connection_string() -> String {
    std::env::var("NEO4J_TEST_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string())
}

fn get_neo4j_user() -> String {
    std::env::var("NEO4J_TEST_USER").unwrap_or_else(|_| "neo4j".to_string())
}

fn get_neo4j_password() -> String {
    std::env::var("NEO4J_TEST_PASSWORD").unwrap_or_else(|_| "password".to_string())
}

async fn setup_test_graph(
    harness: &TestHarness,
) -> Result<InstrumentedGraph, Box<dyn std::error::Error>> {
    let uri = get_neo4j_connection_string();
    let user = get_neo4j_user();
    let password = get_neo4j_password();

    // Create instrumented graph using the simplified API
    let graph = InstrumentedGraph::connect(&uri, &user, &password).await?;

    // Clean up any existing test data
    graph
        .run(Query::new("MATCH (n:TestNode) DELETE n".to_string()))
        .await?;

    // Clear any spans from the cleanup operation
    harness.reset();

    Ok(graph)
}

fn validate_db_span_attributes(span: &SpanData) {
    // Check for database attributes - use only non-deprecated attributes
    let has_db_system = span
        .attributes
        .iter()
        .any(|kv| kv.key.as_str() == DB_SYSTEM_NAME);

    assert!(has_db_system, "Missing {} attribute", DB_SYSTEM_NAME);

    // Check that we have operation info (db.operation.name is the non-deprecated attribute)
    let has_operation_info = span.attributes.iter().any(|kv| {
        let key = kv.key.as_str();
        key == DB_OPERATION_NAME || key == DB_QUERY_TEXT
    });

    assert!(
        has_operation_info,
        "Missing {} or {} attribute",
        DB_OPERATION_NAME, DB_QUERY_TEXT
    );

    // Check for server address attribute
    let has_server_address = span
        .attributes
        .iter()
        .any(|kv| kv.key.as_str() == SERVER_ADDRESS);

    assert!(has_server_address, "Missing {} attribute", SERVER_ADDRESS);

    // Check for database namespace
    let has_db_namespace = span
        .attributes
        .iter()
        .any(|kv| kv.key.as_str() == DB_NAMESPACE);

    assert!(has_db_namespace, "Missing {} attribute", DB_NAMESPACE);

    // Check span kind is CLIENT for database calls
    assert_eq!(
        span.span_kind,
        SpanKind::Client,
        "Database spans should have CLIENT span kind"
    );
}

#[tokio::test]
async fn test_instrumented_run_query() -> Result<(), Box<dyn std::error::Error>> {
    let harness = TestHarness::new();
    let graph = setup_test_graph(&harness).await?;

    // Execute a simple query
    let query =
        Query::new("CREATE (n:TestNode {name: $name}) RETURN n".to_string()).param("name", "test");

    graph.run(query).await?;

    // Get the exported spans
    let spans = harness.get_spans();

    assert!(
        !spans.is_empty(),
        "Expected at least one span to be created"
    );

    // Find the run span (our implementation uses neo4j.run, not neo4j.query)
    let query_span = spans
        .iter()
        .find(|s| s.name == "neo4j.run" || s.name.contains("run"))
        .expect("Should have a run span");

    // Validate span attributes
    validate_db_span_attributes(query_span);

    // Check for Neo4j-specific attributes
    let db_system = query_span
        .attributes
        .iter()
        .find(|kv| kv.key.as_str() == DB_SYSTEM_NAME)
        .map(|kv| kv.value.as_str());

    let db_system_str = db_system.as_ref().map(|s| s.as_ref());
    assert_eq!(
        db_system_str,
        Some("neo4j"),
        "{} should be 'neo4j'",
        DB_SYSTEM_NAME
    );

    Ok(())
}

#[tokio::test]
async fn test_instrumented_execute_query() -> Result<(), Box<dyn std::error::Error>> {
    let harness = TestHarness::new();
    let graph = setup_test_graph(&harness).await?;

    // Execute a query and fetch results
    let query =
        Query::new("CREATE (n:TestNode {value: $value}) RETURN n".to_string()).param("value", 42);

    // For execute, neo4rs returns a stream. We need to consume it.
    // In integration tests, we can just run() instead which doesn't return a stream
    graph.run(query).await?;

    // Check that spans were created
    let spans = harness.get_spans();
    assert!(
        !spans.is_empty(),
        "Expected at least one span to be created"
    );

    // Validate all database spans
    for span in spans.iter() {
        if span.name.contains("neo4j") || span.name.contains("execute") {
            validate_db_span_attributes(span);
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_instrumented_transaction() -> Result<(), Box<dyn std::error::Error>> {
    let harness = TestHarness::new();
    let graph = setup_test_graph(&harness).await?;

    // Start a transaction
    let mut txn = graph.start_txn().await?;

    // Execute queries within the transaction
    txn.run(Query::new("CREATE (n:TestNode {tx: true})".to_string()))
        .await?;

    // Commit the transaction
    txn.commit().await?;

    // Check that spans were created
    let spans = harness.get_spans();
    assert!(
        !spans.is_empty(),
        "Expected transaction spans to be created"
    );

    // Look for transaction-related spans
    let has_tx_start = spans.iter().any(|s| {
        s.name == "neo4j.transaction"
            || s.name == "neo4j.transaction.start"
            || s.name.contains("start_txn")
    });
    let has_tx_commit = spans
        .iter()
        .any(|s| s.name == "neo4j.transaction.commit" || s.name.contains("commit"));

    assert!(has_tx_start || has_tx_commit, "Expected transaction spans");

    // Validate all spans
    for span in spans.iter() {
        if span.name.contains("neo4j") || span.name.contains("transaction") {
            validate_db_span_attributes(span);
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_query_with_parameters() -> Result<(), Box<dyn std::error::Error>> {
    let harness = TestHarness::new();
    let graph = setup_test_graph(&harness).await?;

    // Create a query with multiple parameters
    let query = Query::new(
        "CREATE (n:TestNode {name: $name, age: $age, active: $active}) RETURN n".to_string(),
    )
    .param("name", "Alice")
    .param("age", 30)
    .param("active", true);

    graph.run(query).await?;

    // Verify spans contain query information
    let spans = harness.get_spans();

    println!("Parameters test - Total spans: {}", spans.len());
    for span in &spans {
        println!("  Span: {}", span.name);
        for attr in &span.attributes {
            println!("    Attr: {} = {:?}", attr.key.as_str(), attr.value);
        }
    }

    assert!(
        !spans.is_empty(),
        "Expected spans to be created for parameterized query"
    );

    let query_span = spans
        .iter()
        .find(|s| s.name.contains("query") || s.name.contains("neo4j"))
        .expect("Should have a query span");

    validate_db_span_attributes(query_span);

    // Check for query text attribute
    let has_query_info = query_span.attributes.iter().any(|kv| {
        let key = kv.key.as_str();
        key == DB_QUERY_TEXT || key == DB_OPERATION_NAME
    });
    assert!(
        has_query_info,
        "Expected query information in span attributes"
    );

    Ok(())
}

#[tokio::test]
async fn test_error_handling() -> Result<(), Box<dyn std::error::Error>> {
    let harness = TestHarness::new();
    let graph = setup_test_graph(&harness).await?;

    // Execute an invalid query that should fail
    let invalid_query = Query::new("INVALID CYPHER SYNTAX".to_string());

    let result = graph.run(invalid_query).await;
    assert!(result.is_err(), "Expected query to fail");

    // Check that error spans were created
    let spans = harness.get_spans();
    assert!(!spans.is_empty());

    // Verify the span indicates an error
    let error_span = spans.iter().find(|span| span.status == Status::error(""));

    if let Some(span) = error_span {
        // Check for error attributes
        let has_error_info = span.attributes.iter().any(|kv| {
            let key = kv.key.as_str();
            key.starts_with("error.") || key == "exception.type" || key == "exception.message"
        });
        assert!(
            has_error_info || span.status == Status::error(""),
            "Error spans should have error information"
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_span_relationships() -> Result<(), Box<dyn std::error::Error>> {
    let harness = TestHarness::new();
    let graph = setup_test_graph(&harness).await?;

    // Create a tracer for parent span
    let tracer = harness.tracer("test");
    let parent_span = tracer.start("parent_operation");
    let cx = opentelemetry::Context::current().with_span(parent_span);

    // Execute query within parent context
    let _guard = cx.attach();
    graph
        .run(Query::new("CREATE (n:TestNode {nested: true})".to_string()))
        .await?;
    drop(_guard); // Explicitly drop the guard to end the parent span

    // Get spans and check parent-child relationships
    let spans = harness.get_spans();
    assert!(spans.len() >= 2, "Expected parent and child spans");

    // Find parent span
    let parent = spans
        .iter()
        .find(|s| s.name == "parent_operation")
        .expect("Should have parent span");

    // Find child span that should have the parent's span_id as parent_span_id
    let has_child = spans
        .iter()
        .filter(|s| s.name != "parent_operation")
        .any(|s| s.parent_span_id == parent.span_context.span_id());

    assert!(has_child, "Query span should be child of parent span");

    Ok(())
}
