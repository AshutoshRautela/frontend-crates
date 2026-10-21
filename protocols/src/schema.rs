// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Schema-compatible re-exports of native async-openai types.
//!
//! These are now the real async-openai types, with native `ToSchema` implementations.
//! Owned protocol fields refer directly to their runtime types. Prefer importing
//! protocol types from [`crate::types`] in new code.

pub use async_openai::types::chat::{
    ChatCompletionAudio, ChatCompletionFunctionCall, ChatCompletionFunctions,
    ChatCompletionRequestAssistantMessageAudio, ChatCompletionRequestAssistantMessageContent,
    ChatCompletionRequestDeveloperMessage, ChatCompletionRequestFunctionMessage,
    ChatCompletionRequestMessageContentPartAudio, ChatCompletionRequestMessageContentPartText,
    ChatCompletionRequestSystemMessageContent, ChatCompletionResponseMessageAudio, CompletionUsage,
    FinishReason, FunctionObject, FunctionType, ImageDetail, PredictionContent, Prompt,
    ResponseFormat, ResponseModalities, Role, ServiceTier, TopLogprobs, WebSearchOptions,
};

/// Describe optional token biases using Dynamo's numeric range, including fractions.
///
/// Used by both owned request schemas because upstream advertises integer biases.
/// This replaces the whole property, including nullability and description; it
/// does not validate or alter the underlying `HashMap<String, Value>` at runtime.
pub(crate) fn logit_bias() -> utoipa::openapi::schema::Object {
    use utoipa::openapi::schema::{ObjectBuilder, SchemaType, Type};

    ObjectBuilder::new()
        .schema_type(SchemaType::from_iter([Type::Object, Type::Null]))
        .description(Some(
            "Map token IDs to numeric logit biases from -100 to 100. Dynamo accepts \
             fractional biases as well as integers. Token ID parsing and validation \
             are performed by the serving layer.",
        ))
        .additional_properties(Some(
            ObjectBuilder::new()
                .schema_type(Type::Number)
                .minimum(Some(-100))
                .maximum(Some(100)),
        ))
        .build()
}
