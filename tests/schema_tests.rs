use jv::schema::{SchemaOverview, SharedStruct};

// Public tests — run against committed sample data in samples/public/
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/samples/public/schema_tests.rs"));
