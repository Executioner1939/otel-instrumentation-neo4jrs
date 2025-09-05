#!/bin/bash

# This script simulates the CI workflow locally
set -e

echo "=== Simulating CI Workflow Locally ==="
echo ""

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Function to print status
print_status() {
    if [ $1 -eq 0 ]; then
        echo -e "${GREEN}✓${NC} $2"
    else
        echo -e "${RED}✗${NC} $2"
        exit 1
    fi
}

echo "1. Running format check..."
cargo fmt --all --check
print_status $? "Format check"

echo ""
echo "2. Running clippy..."
cargo clippy --all-features --all-targets -- -D warnings
print_status $? "Clippy"

echo ""
echo "3. Running cargo check..."
cargo check --all-features --all-targets
print_status $? "Cargo check"

echo ""
echo "4. Running unit tests (in parallel)..."
cargo test --all-features --lib
print_status $? "Unit tests"

echo ""
echo "5. Running doc tests..."
cargo test --doc --all-features
print_status $? "Doc tests"

echo ""
echo "6. Checking if Neo4j is available for integration tests..."
if nc -z localhost 7687 2>/dev/null; then
    echo "Neo4j is available at localhost:7687"
    echo ""
    echo "7. Running integration tests (sequentially)..."
    NEO4J_TEST_URI=bolt://localhost:7687 \
    NEO4J_TEST_USER=neo4j \
    NEO4J_TEST_PASSWORD=password \
    cargo test --features integration --test integration_test -- --test-threads=1
    print_status $? "Integration tests"
else
    echo "Neo4j is not available. Skipping integration tests."
    echo "To run integration tests, start Neo4j with:"
    echo "  docker run -d --name neo4j-test \\"
    echo "    -p 7687:7687 -p 7474:7474 \\"
    echo "    -e NEO4J_AUTH=neo4j/password \\"
    echo "    -e NEO4J_ACCEPT_LICENSE_AGREEMENT=yes \\"
    echo "    neo4j:5-community"
fi

echo ""
echo "8. Building documentation..."
cargo doc --all-features --no-deps
print_status $? "Documentation"

echo ""
echo -e "${GREEN}=== All CI checks passed! ===${NC}"