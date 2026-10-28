# Porting a model family to the UnifiedParser

The unified parser is ONE state machine per stream that owns reasoning, visible content, and tool calls, and emits ONE ordered event list. A legacy split pipeline runs a reasoning parser over the stream first, then a tool parser on the leftover — which cannot represent WHERE reasoning happened, so every thought is hoisted to the front and merged. See [`../../conformance/utils/lib/parsers/UNIFIED_CASES.md`](../../conformance/utils/lib/parsers/UNIFIED_CASES.md) for what that costs, case by case.

This doc is for adding a family to the Unified path. The [README registry inventory](README.md#parser-families) and `unified_registry!` in `src/unified/mod.rs` identify current families and aliases. Serving integrations select from their pinned crate version; library support alone does not qualify a model deployment. `beam` uses the shared scanner's `ChannelEnvelope` mode, while Muse and Harmony have channel-specific shared owners. Extend the owner that matches the grammar before adding state.

## What you get for free

`ScannerUnified` (`src/unified/mod.rs`) is the shared body. It is generic over the emitter and holds a `WrappedBlockScanner`, and the whole `UnifiedParser` impl is family-blind: `initialize`, `initialize_request`, `parse_into`, `finish`, `reset`.

Everything below is shared by `src/tool_calling/scan.rs` and the guided router in `src/unified/mod.rs` — do NOT reimplement any of it per family:

- block open/close, bare-invoke recovery, orphan-close stripping
- chunk-boundary marker holdback (`marker_prefix_suffix_len`)
- reasoning open/close, and EOF promotion of an unterminated thought
- tool-open break-out from INSIDE a thought, and resume after the call closes (invariant `I3`; case `12-2`)
- guided-JSON decoding, including the reasoning envelope around the payload
- a grammar-aware invoke scan for families whose markers cannot delimit a call on their own (`InvokeScan`)
- an optional structural role label after the reasoning opener (`ReasoningSpec::start_label`)

The Gemma4 port added invoke scanning and optional reasoning labels. Extend these shared hooks before adding a family-specific drain loop.

Guided decoding is a BACKEND feature, not a model feature — any family can be served with it — which is why `GuidedState` lives on the shared type and is parameterized on the family's `ReasoningSpec` markers rather than on any one grammar. A family that joins the unified path inherits it.

## Conversion contract

“Port this family to UnifiedParser” means **replace the family’s v2 parser state machine**, not add a second route beside it. `UnifiedParser` plus its shared scanner must own all incremental state: buffers, marker holdback, current invoke, recovery latches, tool indices, reasoning channel state, request initialization, `finish`, and `reset`.

- Delete the old family-specific drain loop and its private scanner state.
- Extract or extend one shared scanner builder, then have the Unified factory use it.
- If the public `ToolParser` registry still needs the family, keep a compatibility adapter only. It may translate `UnifiedParserEvent` events into `ToolParseResult`, but it must not parse DSML/model text itself or retain a second buffer, marker set, recovery policy, or argument decoder.
- Test the compatibility adapter against the UnifiedParser on the same complete input and every valid streaming split. The two API surfaces may regroup deltas only where the legacy wire contract requires it; their final text and tool calls must agree.

A registry entry or `unified/<family>.rs` factory without this deletion is a partial experiment, not a conversion.

## Adding a family: parser edits

**1. Write `parsers/v2/src/unified/<family>.rs`** — a factory over the shared `ScannerUnified`. `unified/qwen3.rs` is the reference; it wires the shared scanner and guided router.

See `src/unified/qwen3.rs` for a wrapped-block factory over `ScannerUnified` and `GuidedRouted`; it supplies `ReasoningSpec` plus family-specific guided-prefix resolution. `ReasoningSpec` includes more than start/end markers, so copy the current constructor shape from source rather than a reduced struct literal. Channel families use their existing shared owner.

It needs two things from the tool-only side, both `pub(crate)`: the emitter type, and a `<family>_scanner(tools)` builder. **Extract that builder first if it does not exist.** Most families construct their scanner inline inside `new()`, so the unified factory cannot reuse it without copying the spec — and a copied spec is a spec that drifts. Have BOTH callers use the one builder.

**2. Add one line to `unified_registry!`** in `unified/mod.rs`:

```rust
unified_registry! {
    "qwen3" | "qwen3_coder" => qwen3::qwen3_unified,
    "<family>"              => <family>::<family>_unified,
}
```

Aliases after the `|` — use them when the corpus name differs from the tool registry name. The macro generates both the constructor match and `REGISTERED_UNIFIED_FAMILIES`, so they cannot disagree.

That is the registry portion of the parser change. The conversion contract above still requires removal of any old family-specific state machine and its replacement with a compatibility projection when callers require the legacy trait.

## Adding a family to the conformance tab: one row

`conformance/utils/src/parser_families.yaml`, under `unified:`:

```yaml
  <family>:
    registry: <family>          # key into families:/markers: — omit if identical
    native: true                # false while it still runs on the SPLIT path
    reasoning_parser: <v1 name> # used by the split path
    tool_parser: <v2 name>      # used by the split path
    golden_spec: <family>.yaml
    leak_markers: []            # markup the shared leak list cannot see
```

That row feeds the golden generator, both capture harnesses, the leak check and the colorizer. `manifest_and_parser_registry_agree_on_native_families` fails if this row and `unified_registry!` disagree in either direction, so the two declarations stay honest.

Then add the family's inputs to `gen_unified_golden.py` and run the pipeline (see Checklist).

## Ranking: how much work is a given family

For wrapped-block families, first reuse `WrappedBlockScanner`, its invoke boundary/emitter hooks, and the guided router. Qwen, DeepSeek V4/V4.1, GLM, Gemma, and Kimi already have Unified factories; their original porting work is not a pending task. MiniMax M2/M3 have tool-only parsers and need a full conversion under the contract above. Channel grammars need their existing channel owner rather than a delimiter-pair approximation; Harmony now has a Unified implementation accepting text or token IDs, so inspect `input_capabilities()` before feeding input.

Families without a Unified registry entry, including Granite, Inkling, Mistral, Step, and legacy append-think, require new grammar/initialization work. A reasoning parser name in v1 alone does not make that family available through Unified.

## The reasoning half is the easy half

This is the opposite of what it looks like. `ReasoningSpec` configures a marker pair, optional role label, forced start, and special-token preservation — and most families fit it directly: `<think>`/`</think>` for qwen3, kimi, minimax_m2, deepseek, glm45; `<mm:think>` for minimax_m3; `[THINK]` for mistral.

You do NOT need a new field for v1's `with_tool_start_token`: `drain_reasoning` already derives tool break-out from `spec.block_starts` and `spec.invoke_start`. The unified semantics are strictly better — v1 exits reasoning permanently at a tool marker, unified suspends and RESUMES after the call closes, which is what case `12-2` requires.

Families that genuinely do not fit a marker pair:

- **harmony / gpt_oss** — the existing Unified Harmony owner handles channels and token IDs.
- **granite** — two alternative starts and two alternative ends; needs a list on both sides.
- **inkling** — a six-state machine over eight markers.
- **muse_glimmer** — no pair exists; the recipient in a dynamic header picks the channel.
- **muse_spark** — inherits the Muse Glimmer channel grammar and additionally routes the reserved `commentary` recipient to visible text.

**gemma4 used to be on that list** and no longer is. Its opener is `<|channel>` plus a
`thought\n` role label, and the label is OPTIONAL. Folding it into `start` parses the
whole corpus — which is why it looks acceptable — but then a bare `<|channel>` matches
nothing and its markup surfaces as visible text, a leak (`I3`) in exactly the case the
narrowing was supposed to tolerate. `ReasoningSpec::start_label` keeps the label
structural and optional, so policy `P4` is IMPLEMENTED rather than deferred. Take the
same route for `granite`: extend the spec, do not narrow the grammar to the corpus.

## Traps

**Audit the emitter for `I7` before claiming the goldens.** qwen3's emitter was changed to call `parse_tool_call_block` directly because re-wrapping the body in `<tool_call>` made the batch parser truncate a value at an embedded `</tool_call>` — an argument value is DATA and must survive byte-exact. gemma4 hit the same wall from the other side: handing its block back to the whole-message extractor re-derived bounds the scanner had already resolved, and that extractor also refuses a call missing BOTH its opener (consumed as block markup) and its closer (never streamed), which is exactly case `5-2`. Its emitter now calls `parse_one_tool_call_gemma4` on the delimited invoke. **Rule: once the scanner has delimited an invoke, the emitter TYPES it — it never re-finds it.** Audit every emitter and compatibility projection against `7-2` (`arg_marker_in_string`) rather than assuming that a delimiter-safe scanner makes a second extraction safe.

**Dangling-end recovery differs.** A `</think>` with nothing open is stripped by the shared scanner and the preceding bytes become TEXT; v1's `with_dangling_end_recovery` classifies them as REASONING. Affects `minimax_m3` and `kimi_k3`. Passing `UnifiedParserStartingState::Reasoning` resolves it, which means the caller has to actually pass it — see below.

**Guided JSON needs a reasoning-aware scanner.** A family registered without a `ReasoningSpec` cannot support `GuidedJson`; `initialize_request` rejects that `UnifiedParserInit` before mutating parser state.

**A guided-decoding case whose input is native markup tests nothing.** Guided
decoding constrains the model to bare JSON, so the payload is grammar-independent and
the family's own markup can never appear in it. gemma4 and kimi_k2 carried NATIVE
markup under `init.tool_output_mode=GuidedJson` for six scenarios — green, because the
families had no unified parser to run them, and unfixable-looking the moment one did.
Render guided inputs once for every family (`every_family` in `gen_unified_golden.py`),
not per family.

**Serving initialization is a separate boundary.** Dynamo consumes Unified parsers from a pinned release. The conformance harness can use `RecoverAsText`; a serving integration should resolve prompt tokens, starting state, tool-output mode, and invalid-guided-payload policy into one `UnifiedParserInit` after prompt rendering and backend request resolution, leaving the default failure policy at `Reject` unless it explicitly supports recovery. Test that object at the serving boundary as well as inside the parser.

## Checklist

1. Extract `<family>_scanner(tools)` and make it the only grammar/state owner. Delete the existing tool-only drain state; if the `ToolParser` registry needs compatibility, replace it with a thin UnifiedParser event projection. Run direct compatibility-projection regressions as well as the latest Unified tests; preserve existing tool-only output unless the authored contract deliberately changes.
2. Add `unified/<family>.rs` with the factory and its `ReasoningSpec`.
3. Add one line to `unified_registry!` in `unified/mod.rs`.
4. Add the `unified:` row in `conformance/utils/src/parser_families.yaml` — use `native: true` when the factory is implemented and remove annotations that still claim split-path behavior.
5. Add the family's inputs to `conformance/utils/src/gen_unified_golden.py` so every scenario gets one in that grammar.
6. Run the [current parser tests and qualification](../../conformance/README.md#current-parser-tests-and-qualification), including the latest `unified_render` and `unified_parity` targets described in [README.md](README.md#unified-parser-validation).
7. Check argument fidelity (`7-2`), adversarial nesting (`12-1`/`12-2`), every valid UTF-8 split, prompt-prefilled channels, guided modes, `finish`, reset/reuse, and compatibility-projection parity.
8. Generate the [latest report](../../conformance/README.md#generate-the-latest-or-history-report) with the Rust publisher, refresh `conformance/CONFORMANCE_v2.html`, and inspect affected inputs, GOLDEN, recorded observations, and per-chunk details over HTTP. Missing output is missing evidence; report it.
9. Leave recorded captures/history unchanged. Repository administrators own updates under the [latest capture plan](../../docs/CONFORMANCE-LATEST-PLAN.md), including changed-request bindings and peer recaptures. Do not bump dependencies or run historical capture/package commands as a contributor prerequisite.
