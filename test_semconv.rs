use opentelemetry_semantic_conventions as semconv;

fn main() {
    println!("DB_SYSTEM_NAME: {}", semconv::attribute::DB_SYSTEM_NAME.as_str());
    println!("DB_OPERATION_NAME: {}", semconv::attribute::DB_OPERATION_NAME.as_str());
}
