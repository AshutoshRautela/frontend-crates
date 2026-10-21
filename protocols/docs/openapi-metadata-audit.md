<!--
SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# Native OpenAPI metadata audit

The `protocol-schema` feature uses native async-openai schemas for imported
chat/completion types. Locally owned types still need their own annotations:
changing a dependency cannot add metadata to a different Rust struct.

This audit covers the 0.42.1 → 0.42.2 dependency update and the schema metadata
on Dynamo-owned OpenAI-derived types. It is not a claim of complete OpenAI API
coverage, identical server behavior, or a reason to remove local type ownership.
The canonical definitions, relaxed inputs, and serving extensions remain intact.

## Sources and method

- Compare the released [async-openai 0.42.1](https://docs.rs/crate/async-openai/0.42.1/source/)
  and [0.42.2 sources](https://docs.rs/crate/async-openai/0.42.2/source/).
- Inventory overlapping definitions with
  `.agents/skills/audit-async-openai-update/scripts/inventory.py`; inspect the
  field types, Serde attributes, schema attributes, and nested type ownership.
  Name equality is only a candidate: `InputMessage` in the Decisions protocol is
  not an OpenAI Responses message, and identical declaration text can refer to
  different locally owned child types.
- Compare restored numeric metadata with Dynamo's existing
  [serving validation](https://github.com/ai-dynamo/dynamo/blob/97cb564246c244a0cd8a5689cbff4c6482ce4203/lib/llm/src/protocols/openai/validate.rs).
  The [OpenAI specification](https://github.com/openai/openai-openapi/blob/manual_spec/openapi.yaml)
  is the source for the standard `n=1` default. Do not treat other OpenAI defaults
  as universal backend/model defaults without separate evidence.
- Test real schema graphs, canonical serialized values, and representative
  accepted/rejected schema values. Run the unchanged wire tests with the feature
  both enabled and disabled.

The inventory found 51 same-name overlaps, including non-OpenAI Decisions names.
Only the two upstream request roots changed within the inventory's declaration
bodies, through schema annotations. Existing upstream runtime definitions and
local Serde behavior remain unchanged. No owned type is removed or narrowed.

## Restored field metadata

These bounds describe serving constraints. They do not make the protocol crate
validate requests: for example, Serde can still parse `n=0`, which the serving
layer rejects. Tests preserve that distinction.

| Owned field | Schema metadata | Relationship to upstream |
| --- | --- | --- |
| Chat/completion `n` | default 1; range 1–128 | Standard default and limits; no Serde value injection |
| Stream options `include_usage`, `continuous_usage_stats` | default false | Existing local Serde normalization; the second flag is a Dynamo extension |
| Chat/completion `temperature` | range 0–2 | Restored upstream limits; no backend-independent default asserted |
| Chat/completion `presence_penalty`, `frequency_penalty` | range -2–2 | Restored upstream limits |
| Chat `top_logprobs` | range 0–20 | Restored upstream limits |
| Completion `logprobs` | range 0–5 | Restored upstream limits |
| Completion `best_of` | range 0–20 | Restored upstream limits; the `best_of >= n` relation remains serving validation |
| Chat/completion `top_p` | greater than 0, at most 1 | Intentional Dynamo difference: upstream includes zero |
| Chat/completion `logit_bias` values | number in -100–100 | Intentional Dynamo difference: fractional values remain valid; upstream advertises integers |

The bias map's string keys remain subject to serving-layer token-ID parsing.
The schema does not attempt to reproduce Rust's `u32` parser with an approximate
regular expression. Cross-field limits, token budgets, supported models, and
backend capabilities remain serving concerns.

## Owned-type decisions

All rows have action **keep**: the dependency upgrade does not authorize a type
representation or runtime compatibility change. “Native children” means fields
now use the upstream type's own schema.

| Type or subtree | Local rationale and metadata decision | Evidence / downstream effect |
| --- | --- | --- |
| `CreateChatCompletionRequest`, `CreateCompletionRequest` | Keep local messages, sampling extensions, strict echo, and extended stream options. Restore the applicable metadata above; register native children. | Request bounds tests; no runtime or builder change |
| `FunctionCall`, `FunctionCallStream`, `ChatCompletionStreamResponseDeltaFunctionCall` | Object arguments normalize to strings; streaming arguments may be absent. Keep the canonical string schemas rather than replacing the permissive parser. | Existing argument wire tests and canonical message schema tests |
| `ChatCompletionMessageToolCall`, `ChatCompletionMessageToolCallChunk` | Keep local function children and continuation-chunk omission. Native `FunctionType` supplies the enum. | Existing continuation and serialized-shape tests |
| `FunctionName`, `ChatCompletionNamedToolChoice`, `ChatCompletionToolChoiceOption`, `ChatCompletionToolType`, `ChatCompletionTool` | Keep function-only choice/tool forms and unwrapped named choices. Native `FunctionObject` describes nested tool definitions. Do not add unsupported upstream custom-tool variants. | Named-choice schema tests; transitive component tests |
| `ImageUrl`, `ChatCompletionRequestMessageContentPartImage` | Keep cached-media/null URL handling and UUID extensions. Native `ImageDetail` retains upstream enum metadata. | Existing media wire tests and multimodal schema test |
| `ChatCompletionRequestUserMessage`, `ChatCompletionRequestUserMessageContent`, `ChatCompletionRequestUserMessageContentPart` | Identical-looking fields still reference local media types. Keep local image/video/audio extensions and native text/audio input children. | Existing media tests; no re-export substitution |
| `ChatCompletionRequestToolMessage`, `ChatCompletionRequestToolMessageContent`, `ChatCompletionRequestToolMessageContentPart` | Keep media-capable tool messages, not upstream text-only children. | Existing tool-message tests; native text component |
| `ChatCompletionRequestSystemMessage` | Keep tools-only system input and dynamic tools. Canonical schema does not express every parser rule. | Existing system-message tests and canonical schema validation |
| `ChatCompletionRequestAssistantMessage`, `ChatCompletionRequestMessage` | Keep reasoning aliases, local tool calls, partial continuations, and local role variants. Native developer/function message schemas are registered. | Existing message/role tests; no new root role added |
| `ReasoningEffort`, `Stop`, `StopReason`, `ReasoningContent` | Preserve local effort values, token-ID stops, backend stop reasons, and reasoning forms. `Stop` remains `anyOf` because empty arrays overlap. | Stop union and wire tests; do not import the four-stop OpenAI limit |
| `ChatCompletionStreamOptions` | Preserve `continuous_usage_stats` and null/missing flags normalizing to false. | Existing flag tests; canonical booleans, not an input-normalization claim |
| `ChatCompletionTokenLogprob`, `ChatChoiceLogprobs` | Preserve token-ID extension and required nullable output keys. `TopLogprobs` now has a native schema. | Existing nullable-output schema tests |
| `ChatCompletionResponseMessage`, `ChatCompletionMessageContent`, response media/content-part types | Preserve multimodal content, reasoning and stop-reason extensions. Do not narrow them to upstream text-only output. | Canonical unary response and media tests |
| `ChatChoice`, `ChatChoiceStream`, `ChatCompletionStreamResponseDelta` | Preserve local response/delta types and required nullable choice fields. Native role/finish enums replace placeholders. | Unary/stream nullable-field schema tests |
| `CreateChatCompletionResponse`, `CreateChatCompletionStreamResponse` | Preserve omission behavior and the custom usage serializer. Native usage, service-tier and nested detail schemas replace placeholders. | Canonical response tests; configured HTTP field projection remains downstream |
| `CreateResponse`, `InputParam`, `InputItem`, `Item`, `MessageItem`, `EasyInputMessage`, `EasyInputContent` | Keep the relaxed Responses input chain. Released upstream has no Responses `ToSchema` surface to integrate. | Existing Responses wire tests; explicitly outside this export's coverage |
| `InputContent`, `InputImageContent`, `InputMessage`, `FunctionCallOutput`, `FunctionCallOutputItemParam` | Keep local child types and missing/null image-detail normalization. | Existing image/tool-output tests; no Responses schema claim |
| `InputOutputMessage`, `InputOutputMessageContent`, `InputOutputTextContent`, `InputReasoningItem` | Preserve input-only names and relaxed IDs/status/annotations. Same-name inventory alone misses these renamed counterparts. | Existing relaxed-input tests; output remains upstream-owned |
| `CountInputTokensRequest`, `CountInputTokensResponse` | Keep the local input-token-count contract and custom null/tool parsing. | Existing token-count tests; not a supported schema root |
| `EmbeddingVector`, `Embedding`, `CreateEmbeddingResponse` | Preserve float-array/base64 output union. Upstream 0.42.2 does not provide embedding schemas. | Existing embedding serialization tests; no schema coverage claim |
| Realtime extensions and `DynamoRealtimeClientEvent` | Local append/commit/clear events are extensions, not lost standard fields. Upstream Realtime schemas are unavailable. | Existing event tests; no schema coverage claim |
| Stream aliases; Decisions/SystemOne types | Streams are transport aliases. Decisions/SystemOne are independent contracts; same-name matches are not evidence of inherited OpenAI semantics. | Keep their current schemas; outside this metadata restoration |

Descriptions and constraints must continue to follow the local semantic owner.
This audit does not mechanically copy upstream Rustdoc: references to hosted
OpenAI models, storage, quotas, or defaults can be inaccurate for Dynamo.

## Native schema adoption and downstream work

Native definitions use `async_openai.*` component names and register their
transitive references. `schema::*` remains a
compatibility import path for these actual types. Constructing the former public
unit placeholders is no longer supported; this is a source-breaking change for
callers that used those constructors.

`CreateCompletionResponse` is now available directly through its re-exported
type. Upstream response schemas can still be more permissive than serialized
output, especially for nullable `Option` fields; native coverage is not proof of
exact output-requiredness. Local stricter response annotations are retained.

After a containing crate release, coordinate these Dynamo changes through
[#351](https://github.com/ai-dynamo/frontend-crates/issues/351):

1. Update the released protocol dependency and both lockfiles. Verify the schema
   feature graph without enabling the async-openai HTTP client.
2. Replace Dynamo's `CompletionResponseSchema` import stub with the real type.
   Preserve aliases, request extensions, reasoning-key configuration, and SSE
   `itemSchema`; check every referenced component is registered.
3. Retire obsolete schema composition. Never overwrite native definitions with
   external schemas. Make the supported protocol versions explicit.
4. Compare native HTTP/file exports, response requiredness, and serialized fixtures;
   update comparison/report fixtures and pinned assessment recipes as needed.

These downstream changes are not performed in this PR. Dynamo's explicit HTTP
choice-count normalization in #16233 remains independent of schema release adoption.

External PRs #425 (tool-message name/image detail) and #414 (root role) overlap
this source area. Their unmerged runtime changes are not imported by this audit;
when either lands, preserve its behavior and rerun the native-schema tests.
