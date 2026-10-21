// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Check schema generation without features supplied by the parent workspace.
//! Run with this directory's Cargo manifest. Success prints the OpenAPI document
//! to stdout. Schema assertions live in `tests/protocol_schema.rs`.

use dynamo_protocols::types::{
    CreateChatCompletionRequest, CreateChatCompletionResponse, CreateChatCompletionStreamResponse,
    CreateCompletionRequest, CreateCompletionResponse,
};
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

/// Print schemas for the five registered request/response types as JSON.
fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&Contract::openapi()).unwrap()
    );
}
