// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Re-exports of async-openai types with native `ToSchema` implementations.
//!
//! These paths remain available for existing schema consumers. Import protocol
//! types from [`crate::types`] in new code; the same types support schema generation.

pub use async_openai::types::chat::{
    ChatCompletionAudio, ChatCompletionFunctionCall, ChatCompletionFunctions,
    ChatCompletionRequestAssistantMessageAudio, ChatCompletionRequestAssistantMessageContent,
    ChatCompletionRequestDeveloperMessage, ChatCompletionRequestFunctionMessage,
    ChatCompletionRequestMessageContentPartAudio, ChatCompletionRequestMessageContentPartText,
    ChatCompletionRequestSystemMessageContent, ChatCompletionResponseMessageAudio, CompletionUsage,
    FinishReason, FunctionObject, FunctionType, ImageDetail, PredictionContent, Prompt,
    ResponseFormat, ResponseModalities, Role, ServiceTier, TopLogprobs, WebSearchOptions,
};

/// Build the schema for a nullable map of token IDs to biases in `[-100, 100]`.
///
/// Both request types use this with `schema_with` because Dynamo accepts fractional
/// biases, while upstream declares integers. The returned schema includes the
/// property's nullability and description, which `schema_with` replaces as well.
/// It does not parse token IDs, validate requests, or change stored values.
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
