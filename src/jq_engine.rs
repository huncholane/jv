use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};

pub struct JqEngine;

#[derive(Debug)]
pub struct JqResult {
    pub output: Vec<String>,
    pub error: Option<String>,
}

impl JqEngine {
    pub fn execute(query: &str, input: &serde_json::Value) -> JqResult {
        if query.trim().is_empty() {
            return JqResult {
                output: vec![serde_json::to_string_pretty(input).unwrap_or_default()],
                error: None,
            };
        }

        let mut defs = ParseCtx::new(Vec::new());
        defs.insert_natives(jaq_core::core());
        defs.insert_defs(jaq_std::std());

        let (filter, errs) = jaq_parse::parse(query, jaq_parse::main());

        if !errs.is_empty() {
            return JqResult {
                output: Vec::new(),
                error: Some(format!("Parse error in query")),
            };
        }

        let Some(filter) = filter else {
            return JqResult {
                output: Vec::new(),
                error: Some("Failed to parse query".to_string()),
            };
        };

        let filter = defs.compile(filter);

        let inputs = RcIter::new(core::iter::empty());
        let val = Val::from(input.clone());

        let mut output = Vec::new();
        for result in filter.run((Ctx::new([], &inputs), val)) {
            match result {
                Ok(v) => {
                    let json_val: serde_json::Value = v.into();
                    output.push(
                        serde_json::to_string_pretty(&json_val)
                            .unwrap_or_else(|_| format!("{:?}", json_val)),
                    );
                }
                Err(e) => {
                    return JqResult {
                        output,
                        error: Some(format!("{}", e)),
                    };
                }
            }
        }

        JqResult {
            output,
            error: None,
        }
    }
}
