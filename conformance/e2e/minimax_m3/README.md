# MiniMax M3 parser replay

`cases.yaml` is the authored request/expectation suite. Each of its 23 scenarios runs with reasoning enabled/disabled and streaming/nonstreaming responses, against four deployed configurations: structural tags off/on and engine stream interval 1/20. This produces **368 checks per implementation**.

Checks cover exact arguments and JSON Schema types, tool choice, parallel call order and IDs, tool-result history, reasoning boundaries, terminal SSE events, finish reasons, Unicode/escaping, and argument delivery before a long string finishes. Expected arguments remain independent of recorded output.

```bash
python3 -m venv .venv
.venv/bin/pip install -r conformance/e2e/minimax_m3/requirements.txt
cargo build --locked -p dynamo-parsers-v2 --example m3_structural_tag

.venv/bin/python conformance/e2e/minimax_m3/run.py \
  --implementation v2 --url http://localhost:8000 \
  --structural-tags off --stream-interval 1 \
  --concurrency 4 --output /tmp/m3-results
```

Set `--implementation` to `v1`, `v2`, or `sglang`. The tags and interval options record the **actual server configuration**; they do not change its engine settings. Restart the engine with the matching stream interval and structural-tag setting before replaying that configuration. Dynamo frontends select their parser through `DYN_PARSER_VERSION=1` or `2`.

For native SGLang with tags enabled, also pass:

```bash
--tag-builder "$PWD/target/debug/examples/m3_structural_tag"
```

The exporter calls the production Rust builder and uses an external reasoning boundary because native SGLang already gates constrained decoding until reasoning ends. The deployed validation uses XGrammar 0.2.8. `--case ID` selects a smoke test; keep smoke captures in a separate output directory.

Every run writes an append-only JSONL journal immediately and a uniquely named JSON capture on completion, including actual requests, assembled responses, raw SSE events and timestamps. Failed iterations are retained. The report selects the latest capture for each implementation/tags/interval combination. Regenerate it with:

```bash
.venv/bin/python conformance/e2e/minimax_m3/run.py \
  --report-only --output /tmp/m3-results
```

The separate hermetic corpus is `conformance/fixtures-unified-v2/families/minimax_m3`: its 236 authored cases also run under every character split, character-sized delivery, and widths 1/20. It tests native and guided parsing independently of model generation.

On a Linux environment with XGrammar 0.2.8, verify that native parallel invocations share one wrapper and that required/type/single-call constraints reject invalid output:

```bash
.venv/bin/pip install xgrammar==0.2.8
.venv/bin/python conformance/e2e/minimax_m3/check_grammar.py \
  --tag-builder target/debug/examples/m3_structural_tag
```

M3's native XML cannot preserve every JSON Schema constraint. Unsupported unions and constrained strings retain the existing JSON-guidance path; tagged replay still checks the original schema and expected arguments.
