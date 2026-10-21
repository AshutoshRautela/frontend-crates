# dynamo-protocols

Request/response types for OpenAI- and Anthropic-compatible inference servers. Built on [`async-openai`](https://crates.io/crates/async-openai) v0.42, with selective overrides where inference engines need behaviors upstream doesn't support.

## What's included

- **Chat Completions** — multimodal content, reasoning content (DeepSeek-R1 / QwQ), continuous usage stats, tool-calling.
- **Batch API** — re-exported from upstream.
- **Files API** — re-exported from upstream.
- **Responses API** — input chain owned (relaxed for Codex / Agents SDK); output chain re-exported from upstream.
- **Completions** — locally owned request; response types re-exported from upstream.
- **Anthropic Messages** — fully owned (no upstream equivalent).
- **Embeddings**, **Images** — re-exported.

## Optional protocol schemas

Enable `protocol-schema` (or its `utoipa` alias) to obtain utoipa 5 schemas for
chat/completion requests, chat responses and stream chunks, and the re-exported
`CreateCompletionResponse`. The feature enables `async-openai/openapi` using
released async-openai 0.42.2 or newer compatible versions. It does not enable the
HTTP client or change Serde behavior.

```rust,ignore
#[derive(utoipa::OpenApi)]
#[openapi(components(schemas(
    dynamo_protocols::types::CreateChatCompletionRequest,
    dynamo_protocols::types::CreateCompletionRequest,
    dynamo_protocols::types::CreateChatCompletionResponse,
    dynamo_protocols::types::CreateChatCompletionStreamResponse,
    dynamo_protocols::types::CreateCompletionResponse
)))]
struct Contract;
```

Owned components keep their `dynamo_protocols.*` names. Imported components use
the real `async_openai.*` schemas, including transitive types such as prompt
variants, tool definitions, response formats, completion choices, and usage.
The existing `schema::*` paths now re-export actual upstream types; they no longer
name placeholder unit structs.

Consumers should use these native components without replacing them with external
definitions. The coordinated Dynamo export/composition migration is tracked in
[#351](https://github.com/ai-dynamo/frontend-crates/issues/351).

### Contract and coverage limits

- **Metadata is not runtime validation.** Numeric limits describe the serving
  contract, but Serde still accepts values representable by the Rust types.
  Likewise, `n: default=1` does not inject a value: omitted/null input remains
  `None` until the server resolves it. Other omitted sampling controls remain
  backend/model dependent; no universal default is added for them.
- **Intentional differences remain.** Dynamo's `top_p` excludes zero and its
  logit biases may be fractional. Local reasoning, multimodal, token-stop,
  streaming-usage, and permissive-input extensions retain their own schemas.
  The [metadata audit](docs/openapi-metadata-audit.md) records what was restored,
  what differs, and what remains outside coverage.
- **Canonical shape is not every accepted input.** Custom deserializers normalize
  object-valued function arguments, media shorthand, reasoning aliases, and null
  stream-option flags. Canonical schemas are not a replacement for those parsers
  or server cross-field validation.
- **Other API families remain uncovered.** This feature does not provide complete
  schemas for Responses, embeddings, images, Realtime, Batch, Files, or Anthropic.
  Released async-openai schema support is limited to chat/completion types and
  their shared dependencies; local definitions in other families are not silently
  presented as covered.
- **Transport and backend behavior are separate.** The component graph does not
  define endpoint registration, HTTP errors, SSE framing/order/termination,
  configured output keys, or actual engine support. Reference closure is not a
  whole-server compatibility claim.

Run the protocol suite in both feature configurations:

```sh
cargo test -p dynamo-protocols --features protocol-schema --locked
cargo test -p dynamo-protocols --no-default-features --locked
cargo run --manifest-path protocols/tests/schema_consumer/Cargo.toml
```

The standalone consumer prints a native OpenAPI document and checks reference
closure without relying on workspace-wide feature unification.
It resolves compatible released dependencies into its own ignored lockfile;
retain that lockfile with validation evidence when reproducing a run.

## Locally-defined extensions

A few fields extend the upstream `async-openai` schema:

- `reasoning_content` on assistant messages
- `mm_processor_kwargs` on chat-completion requests (vLLM multimodal)
- `continuous_usage_stats` on chat stream options
- `FunctionCall.arguments` accepts both string and object forms

## Request compatibility and serving policy

`CreateResponse` accepts `"text":{"verbosity":"low"}` without a `format`.
The upstream `ResponseTextParam` defaults the format to `text`. An explicit
`text`, `json_object`, or `json_schema` format is preserved independently of
verbosity. A serving adapter that cannot honor verbosity should ignore that
hint while retaining the output-format constraint. Map supported fields to
the engine request instead of forwarding the Responses `text` object.

`CreateChatCompletionRequest` and `CreateCompletionRequest` accept and preserve
`stream_options` when `stream` is absent, null, false, or true. These types do
not enforce cross-field streaming policy. A serving adapter that chooses
permissive compatibility should clear `stream_options` when
`request.stream != Some(true)` before backend validation or forwarding. Keep
the options when streaming, and keep normal non-streaming response usage.
This normalization belongs in the serving adapter because protocol types
also serve callers that need to preserve the original request.

Run the wire-level regressions for
[#299](https://github.com/ai-dynamo/frontend-crates/issues/299) with
`cargo test -p dynamo-protocols --test request_compatibility`.
