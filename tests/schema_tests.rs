use jv::schema::{SchemaOverview, SharedStruct};

// Public tests — run against committed sample data in samples/public/
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/samples/public/schema_tests.rs"));

// Private tests — only compiled if the file exists.
// To use: add JSON/HAR files to samples/private/ and tests to tests/private/schema_tests.rs
#[cfg(feature = "private_tests")]
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/private/schema_tests.rs"));
