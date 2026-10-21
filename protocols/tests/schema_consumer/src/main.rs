// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Check schema generation without features supplied by the parent workspace.
//! Run with this directory's Cargo manifest. Success prints the OpenAPI document
//! to stdout; a failed schema check panics and exits with an error.

use dynamo_protocols::types::{
    CreateChatCompletionRequest, CreateChatCompletionResponse, CreateChatCompletionStreamResponse,
    CreateCompletionRequest, CreateCompletionResponse,
};
use serde_json::Value;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(components(schemas(
    CreateChatCompletionRequest,
    CreateCompletionRequest,
    CreateChatCompletionResponse,
    CreateChatCompletionStreamResponse,
    CreateCompletionResponse
)))]
struct Contract;

/// Check references below `node` against the full OpenAPI `document`.
/// Panics if a reference is outside `components/schemas` or has no definition.
fn assert_references(node: &Value, document: &Value) {
    match node {
        Value::Object(fields) => {
            if let Some(reference) = fields.get("$ref").and_then(Value::as_str) {
                assert!(
                    reference.starts_with("#/components/schemas/"),
                    "{reference}"
                );
                assert!(document.pointer(&reference[1..]).is_some(), "{reference}");
            }
            for child in fields.values() {
                assert_references(child, document);
            }
        }
        Value::Array(values) => {
            for child in values {
                assert_references(child, document);
            }
        }
        _ => {}
    }
}

/// Print schemas for the five registered request/response types after checking
/// references, a nested field shape, completion-response registration, and `n=1`.
fn main() {
    let document = serde_json::to_value(Contract::openapi()).unwrap();
    assert_references(&document, &document);
    assert_eq!(
        document["components"]["schemas"]["async_openai.FunctionObject"]["properties"]["name"]["type"],
        "string"
    );
    for name in [
        "dynamo_protocols.chat.CreateChatCompletionRequest",
        "dynamo_protocols.completion.CreateCompletionRequest",
    ] {
        assert_eq!(
            document["components"]["schemas"][name]["properties"]["n"]["default"],
            1
        );
    }
    assert!(
        document["components"]["schemas"]
            .get("async_openai.CreateCompletionResponse")
            .is_some()
    );
    println!("{}", serde_json::to_string_pretty(&document).unwrap());
}
