// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Streaming XML-ish tool-call parser for MiniMax-M3.
//!
//! MiniMax-M3 prefixes every tag with the namespace token `]<]minimax[>[` and
//! names parameters by their TAG (not a `name=` attribute):
//!   `]<]minimax[>[<tool_call>
//!    ]<]minimax[>[<invoke name="NAME">
//!    ]<]minimax[>[<KEY>value]<]minimax[>[</KEY>
//!    ]<]minimax[>[</invoke>
//!    ]<]minimax[>[</tool_call>`
//! plus a bare `]<]minimax[>[<invoke ...>...</invoke>` back-off form when the
//! outer wrapper is absent.
//!
//! The streaming concern (buffering, chunk-split marker safety, normal_text
//! suppression) is owned by the shared [`scan::WrappedBlockScanner`]; all
//! three markers begin with the namespace token, so the shared holdback also
//! retains a split `]<]minimax[>[` run. The per-invoke value typing is
//! delegated to the v1 batch parser `try_tool_call_parse_minimax_m3` driven by
//! the same config `dynamo_parsers` uses for batch parsing, so a streamed call
//! matches exactly what the batch parser produces. Arguments are re-serialized
//! in source parameter-tag order because the v1 parser builds them from a
//! `HashMap` whose key order is non-deterministic.

use crate::tool_calling::scan::{
    BareRecoveryLatch, GuidedInvokePrefix, GuidedInvokePrefixContext, InvokeBoundary,
    InvokeBoundaryFactory, InvokeEmitter, InvokeLatch, WrappedBlockScanner, WrappedBlockSpec,
    reorder_arguments,
};
use crate::tool_calling::v1core::{
    MiniMaxM3ParserConfig, ToolDefinition, try_tool_call_parse_minimax_m3,
};

use crate::tool_calling::scan::marker_prefix_suffix_len;
use crate::tool_calling::traits::{Tool, ToolCallDelta, ToolParseResult, ToolParser};
use crate::tool_calling::v1core::xml::minimax_m3_parser::{
    html_unescape, parse_invoke_name, preserves_single_parameter, preserves_string_value,
};
use std::collections::HashSet;

/// The namespace token emitted before every M3 tag.
const NS: &str = "]<]minimax[>[";
const BLOCK_START: &str = "]<]minimax[>[<tool_call>";
const BLOCK_END: &str = "]<]minimax[>[</tool_call>";
/// Bare `<invoke` (no trailing `>`): matches both `<invoke name="...">` and the
/// malformed nameless `<invoke>` (whose parse yields no call and is dropped).
const FUNCTION_START: &str = "]<]minimax[>[<invoke";
const GUIDED_FUNCTION_START: &str = "]<]minimax[>[<invoke name=\"";
const FUNCTION_END: &str = "]<]minimax[>[</invoke>";

fn spec() -> WrappedBlockSpec {
    WrappedBlockSpec {
        family: "minimax_m3",
        block_starts: vec![BLOCK_START.to_string()],
        block_ends: vec![BLOCK_END.to_string()],
        invoke_start: FUNCTION_START.to_string(),
        invoke_end: FUNCTION_END.to_string(),
        orphan_markers: vec![BLOCK_END.to_string()],
        // BLOCK_END is held back too: after a bare-invoke recovery latches
        // suppression, a split closing marker must be retained whole so the
        // orphan-close path (which clears the latch) can match it.
        holdback_markers: vec![
            BLOCK_START.to_string(),
            FUNCTION_START.to_string(),
            BLOCK_END.to_string(),
        ],
        bare_recovery_latch: BareRecoveryLatch::Set,
        invoke_latch: InvokeLatch::IfEmitted,
        drop_invoke_crossing_block_end: false,
        // Every wrapped family's markers are special tokens today.
        preserve_special_tokens: true,
        invoke_boundary_factory: Some(InvokeBoundaryFactory::Custom(|| {
            Box::new(M3Boundary::default())
        })),
    }
}

/// Keep header scans incremental; a `>` inside guided JSON belongs to the payload.
#[derive(Default)]
struct M3Boundary {
    header_cursor: usize,
    close_cursor: usize,
    native_header: bool,
}

impl InvokeBoundary for M3Boundary {
    fn guided_invoke_start(&self) -> Option<&'static str> {
        Some(GUIDED_FUNCTION_START)
    }
    fn is_guided_invoke_marker(&self, marker: &str) -> bool {
        marker == FUNCTION_START || marker == GUIDED_FUNCTION_START
    }

    fn owns_guided_prefix(&self) -> bool {
        true
    }

    fn guided_prefix_append(
        &mut self,
        candidate: &str,
        _append: &str,
        context: GuidedInvokePrefixContext,
    ) -> Option<GuidedInvokePrefix> {
        if self.native_header {
            return Some(GuidedInvokePrefix::NoMatch);
        }
        self.header_cursor = self.header_cursor.max(FUNCTION_START.len());
        while self.header_cursor < candidate.len() {
            let ch = candidate[self.header_cursor..].chars().next()?;
            if matches!(ch, '{' | '[') {
                return Some(if context.outside_reasoning {
                    GuidedInvokePrefix::Match(self.header_cursor)
                } else {
                    GuidedInvokePrefix::Strip(self.header_cursor)
                });
            }
            if ch == '>' {
                self.native_header = true;
                return Some(GuidedInvokePrefix::NoMatch);
            }
            if ch == '<' || context.followed_by_competing_marker {
                let prefix = candidate[FUNCTION_START.len()..]
                    .find("name=")
                    .map(|at| FUNCTION_START.len() + at + "name=".len())
                    .map(|at| at + usize::from(candidate[at..].starts_with(['\"', '\''])))
                    .unwrap_or(FUNCTION_START.len());
                return Some(GuidedInvokePrefix::Strip(prefix));
            }
            self.header_cursor += ch.len_utf8();
        }
        Some(if context.followed_by_competing_marker {
            GuidedInvokePrefix::Strip(self.header_cursor)
        } else {
            GuidedInvokePrefix::Pending
        })
    }

    fn end_append(
        &mut self,
        candidate: &str,
        _append: &str,
        _flush: bool,
        _tool_index: usize,
    ) -> Option<usize> {
        let tail = &candidate[self.close_cursor..];
        if let Some(at) = tail.find(FUNCTION_END) {
            return Some(self.close_cursor + at + FUNCTION_END.len());
        }
        self.close_cursor = candidate.len() - marker_prefix_suffix_len(tail, [FUNCTION_END]);
        None
    }
    fn opens(&self, _text: &str, _at: usize) -> bool {
        true
    }
    fn holdback(&self, text: &str) -> usize {
        if !self.native_header && self.header_cursor > 0 {
            text.rfind(FUNCTION_START).map_or(0, |at| text.len() - at)
        } else {
            0
        }
    }
    fn resync(&mut self, text: &str, _flush: bool, _tool_index: usize) -> Option<usize> {
        text.get(1..)?.find(BLOCK_START).map(|at| at + 1)
    }
    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Value-typing hook: wraps one complete invoke run in the M3 `<tool_call>`
/// block so the v1 parser takes its normal wrapped path, then re-orders the
/// arguments to source parameter-tag order.
pub(crate) struct M3Emitter {
    config: MiniMaxM3ParserConfig,
    tools: Vec<ToolDefinition>,
    partial: Option<PartialInvoke>,
}

struct PartialInvoke {
    name: String,
    cursor: usize,
    emitted: String,
    seen: HashSet<String>,
    active: Option<(String, usize)>,
    blocked: bool,
    announced: bool,
}

impl InvokeEmitter for M3Emitter {
    fn parse_partial_invoke(
        &mut self,
        invoke: &str,
        tool_index: usize,
    ) -> anyhow::Result<Option<ToolCallDelta>> {
        if self.partial.is_none() {
            let Some(end) = invoke[FUNCTION_START.len()..]
                .find('>')
                .map(|at| FUNCTION_START.len() + at)
            else {
                return Ok(None);
            };
            let Some(name) = parse_invoke_name(&invoke[FUNCTION_START.len()..end]) else {
                return Ok(None);
            };
            if name.is_empty() {
                return Ok(None);
            }
            self.partial = Some(PartialInvoke {
                name,
                cursor: end + 1,
                emitted: String::new(),
                seen: HashSet::new(),
                active: None,
                blocked: false,
                announced: false,
            });
        }
        let partial = self.partial.as_mut().unwrap();
        let mut arguments = String::new();
        while !partial.blocked {
            if let Some((key, value_cursor)) = &mut partial.active {
                let close = format!("{NS}</{key}>");
                let tail = &invoke[*value_cursor..];
                let end = tail.find(&close);
                let safe = end.unwrap_or_else(|| {
                    tail.len() - marker_prefix_suffix_len(tail, [NS, close.as_str()])
                });
                // Nested XML changes the value's type. Let the final decoder own it.
                if tail[..safe].contains(NS) {
                    partial.blocked = true;
                    break;
                }
                // Retain partial entities, including the batch decoder's amp-to-quote forms.
                let safe = if end.is_none() {
                    safe - marker_prefix_suffix_len(
                        &tail[..safe],
                        [
                            "&lt;",
                            "&gt;",
                            "&amp;",
                            "&quot;",
                            "&#x27;",
                            "&#39;",
                            "&amp;quot;",
                            "&amp;#x27;",
                            "&amp;#39;",
                        ],
                    )
                } else {
                    safe
                };
                let encoded = serde_json::to_string(&html_unescape(&tail[..safe]))?;
                arguments.push_str(&encoded[1..encoded.len() - 1]);
                *value_cursor += safe;
                if end.is_some() {
                    arguments.push('"');
                    partial.cursor = *value_cursor + close.len();
                    partial.active = None;
                    continue;
                }
                break;
            }
            let tail = &invoke[partial.cursor..];
            let Some(at) = tail.find(NS) else {
                partial.cursor = invoke.len() - marker_prefix_suffix_len(tail, [NS]);
                break;
            };
            partial.cursor += at;
            let tag = &invoke[partial.cursor + NS.len()..];
            let Some(end) = tag.find('>') else { break };
            let Some(key) = tag[..end].strip_prefix('<') else {
                partial.blocked = true;
                break;
            };
            let root_schema = self
                .tools
                .iter()
                .find(|tool| tool.name == partial.name)
                .and_then(|tool| tool.parameters.as_ref());
            let schema = root_schema
                .and_then(|root| root.get("properties"))
                .and_then(|properties| properties.get(key));
            let streamable = root_schema
                .zip(schema)
                .is_some_and(|(root, schema)| preserves_string_value(schema, root));
            if !streamable {
                if !root_schema.is_some_and(|root| preserves_single_parameter(schema, root)) {
                    partial.blocked = true;
                    break;
                }
                // A scalar or structured value needs the schema coercer, but
                // must not prevent later string parameters from streaming.
                let close = format!("{NS}</{key}>");
                let value_start = partial.cursor + NS.len() + end + 1;
                let Some(end) = invoke[value_start..].find(&close) else {
                    break;
                };
                let parameter_end = value_start + end + close.len();
                let wrapped = format!(
                    "{BLOCK_START}{}{FUNCTION_END}{BLOCK_END}",
                    &invoke[..parameter_end]
                );
                let tools = (!self.tools.is_empty()).then_some(self.tools.as_slice());
                let (calls, _) = try_tool_call_parse_minimax_m3(&wrapped, &self.config, tools)?;
                let Some(call) = calls.into_iter().next() else {
                    partial.blocked = true;
                    break;
                };
                let typed = reorder_arguments(
                    &call.function.arguments,
                    &source_parameter_order(&invoke[..parameter_end]),
                );
                let prefix = format!("{}{}", partial.emitted, arguments);
                let Some(suffix) = typed
                    .strip_suffix('}')
                    .and_then(|body| body.strip_prefix(&prefix))
                else {
                    partial.blocked = true;
                    break;
                };
                arguments.push_str(suffix);
                partial.seen.insert(key.to_string());
                partial.cursor = parameter_end;
                continue;
            }
            if !partial.seen.insert(key.to_string()) {
                partial.blocked = true;
                break;
            }
            arguments.push(if partial.emitted.is_empty() && arguments.is_empty() {
                '{'
            } else {
                ','
            });
            arguments.push_str(&serde_json::to_string(key)?);
            arguments.push_str(":\"");
            partial.active = Some((key.to_string(), partial.cursor + NS.len() + end + 1));
        }
        let name = (!partial.announced).then(|| partial.name.clone());
        if name.is_none() && arguments.is_empty() {
            return Ok(None);
        }
        partial.announced = true;
        partial.emitted.push_str(&arguments);
        Ok(Some(ToolCallDelta {
            tool_index,
            name,
            arguments,
            complete: false,
        }))
    }

    fn parse_invoke(
        &mut self,
        invoke: &str,
        tool_index: usize,
    ) -> anyhow::Result<Option<ToolCallDelta>> {
        let partial = self.partial.take();
        let wrapped = format!("{BLOCK_START}{invoke}{BLOCK_END}");
        let tools_opt = (!self.tools.is_empty()).then_some(self.tools.as_slice());
        let (calls, _content) = try_tool_call_parse_minimax_m3(&wrapped, &self.config, tools_opt)?;
        let Some(call) = calls.into_iter().next() else {
            return Ok(None);
        };
        let arguments =
            reorder_arguments(&call.function.arguments, &source_parameter_order(invoke));
        let announced = partial.as_ref().is_some_and(|p| p.announced);
        let arguments = if let Some(partial) = partial {
            let Some(suffix) = arguments.strip_prefix(&partial.emitted) else {
                tracing::warn!(
                    why = "minimax_m3_streamed_arguments_invalidated",
                    tool_index
                );
                return Ok(None);
            };
            suffix.to_string()
        } else {
            arguments
        };
        Ok(Some(ToolCallDelta {
            tool_index,
            name: (!announced).then_some(call.function.name),
            arguments,
            complete: true,
        }))
    }

    fn reset(&mut self) {
        self.partial = None;
    }
}

/// Build the single scanner shared by the unified and tool-only entry points.
pub(crate) fn minimax_m3_scanner(tools: &[Tool]) -> WrappedBlockScanner<M3Emitter> {
    WrappedBlockScanner::new(
        spec(),
        M3Emitter {
            config: MiniMaxM3ParserConfig::default(),
            tools: tools.iter().map(ToolDefinition::from).collect(),
            partial: None,
        },
    )
}

/// Compatibility projection of the unified parser with reasoning disabled.
pub struct MiniMaxM3ToolStreamParser {
    parser: Box<dyn crate::unified::UnifiedParser>,
}

impl MiniMaxM3ToolStreamParser {
    pub fn new(tools: &[Tool]) -> Self {
        let mut parser = crate::unified::minimax_m3::minimax_m3_unified(tools);
        parser
            .initialize_request(crate::unified::UnifiedParserInit {
                starting_state: crate::unified::UnifiedParserStartingState::Response,
                ..Default::default()
            })
            .expect("M3 supports the visible channel");
        Self { parser }
    }
}

impl ToolParser for MiniMaxM3ToolStreamParser {
    fn create(tools: &[Tool]) -> anyhow::Result<Box<dyn ToolParser>> {
        Ok(Box::new(Self::new(tools)))
    }

    fn preserve_special_tokens(&self) -> bool {
        self.parser.preserve_special_tokens()
    }

    fn push(&mut self, chunk: &str) -> anyhow::Result<ToolParseResult> {
        use crate::unified::UnifiedParserExt;
        Ok(ToolParseResult::from_deltas(self.parser.push(chunk)?))
    }

    fn finish(&mut self) -> anyhow::Result<ToolParseResult> {
        Ok(ToolParseResult::from_deltas(self.parser.finish()?.events))
    }
}

/// TOP-LEVEL parameter tag names in the order they appear in an invoke run.
///
/// M3 names parameters by their tag (`]<]minimax[>[<location>value...`), and
/// values may nest further namespaced tags (arrays/objects — batch cases
/// 7.d/7.e), so the scan tracks tag depth and records only the depth-0 openers
/// inside the invoke body; nested tag names never shadow a top-level key.
fn source_parameter_order(function: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth: i32 = 0;
    let mut cursor = 0;
    while let Some(rel) = function[cursor..].find(NS) {
        let tag_start = cursor + rel + NS.len();
        let rest = &function[tag_start..];
        let Some(after_lt) = rest.strip_prefix('<') else {
            cursor = tag_start;
            continue;
        };
        if let Some(closer) = after_lt.strip_prefix('/') {
            // `</invoke>` closes the run; any other closer pops one level.
            if !closer.starts_with("invoke") {
                depth -= 1;
            }
            cursor = tag_start + 1;
            continue;
        }
        if after_lt.starts_with("invoke") {
            cursor = tag_start + 1;
            continue;
        }
        let Some(name_end) = after_lt.find('>') else {
            break;
        };
        let name = after_lt[..name_end].trim();
        if !name.is_empty() {
            if depth == 0 {
                names.push(name.to_string());
            }
            depth += 1;
        }
        cursor = tag_start + 1 + name_end + 1;
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weather_tools() -> Vec<Tool> {
        vec![Tool {
            name: "get_weather".to_string(),
            description: None,
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "location": { "type": "string" } }
            }),
            strict: None,
        }]
    }

    fn parse_chunks(tools: &[Tool], chunks: &[&str]) -> ToolParseResult {
        let mut parser = MiniMaxM3ToolStreamParser::new(tools);
        let mut out = ToolParseResult::default();
        for chunk in chunks {
            out.append(parser.push(chunk).expect("push"));
        }
        out.append(parser.finish().expect("finish"));
        out
    }

    #[test]
    fn null_constraints_preserve_argument_types() {
        let input = concat!(
            "]<]minimax[>[<tool_call>]<]minimax[>[<invoke name=\"get_weather\">",
            "]<]minimax[>[<location>null]<]minimax[>[</location>",
            "]<]minimax[>[</invoke>]<]minimax[>[</tool_call>",
        );
        for (schema, expected) in [
            (
                serde_json::json!({"anyOf": [{"type": "string"}, {"const": null}]}),
                "{\"location\":null}",
            ),
            (
                serde_json::json!({"type": "string", "anyOf": [{"type": "null"}, {"type": "string"}]}),
                "{\"location\":\"null\"}",
            ),
        ] {
            let mut tools = weather_tools();
            tools[0].parameters["properties"]["location"] = schema;
            let output = parse_chunks(&tools, &[input]).coalesce_calls();
            assert_eq!(output.calls.len(), 1);
            assert_eq!(output.calls[0].arguments, expected);
            assert!(output.calls[0].complete);
        }
    }

    #[test]
    fn emits_complete_call_on_close() {
        let out = parse_chunks(
            &weather_tools(),
            &[
                "]<]minimax[>[<tool_call>\n]<]minimax[>[<invoke name=\"get_weather\">",
                "\n]<]minimax[>[<location>",
                "NYC]<]minimax[>[</location>\n]<]minimax[>[</invoke>",
                "\n]<]minimax[>[</tool_call>",
            ],
        );
        assert_eq!(out.normal_text, "");
        let merged = out.coalesce_calls();
        assert_eq!(merged.calls.len(), 1);
        assert_eq!(merged.calls[0].tool_index, 0);
        assert_eq!(merged.calls[0].name.as_deref(), Some("get_weather"));
        assert_eq!(merged.calls[0].arguments, r#"{"location":"NYC"}"#);
    }

    #[test]
    fn preserves_prefix_and_trailing_text() {
        // 8.c: prefix before the block AND narration after the close both flow
        // into normal_text verbatim; the block markup is suppressed.
        let out = parse_chunks(
            &weather_tools(),
            &[
                "I will check the weather. ]<]minimax[>[<tool_call>\n]<]minimax[>[<invoke name=\"get_weather\">",
                "\n]<]minimax[>[<location>NYC]<]minimax[>[</location>\n]<]minimax[>[</invoke>\n]<]minimax[>[</tool_call>",
                " Let me know if you need more.",
            ],
        );
        assert_eq!(
            out.normal_text,
            "I will check the weather.  Let me know if you need more."
        );
        assert_eq!(out.coalesce_calls().calls.len(), 1);
    }

    #[test]
    fn suppresses_in_block_narration_between_invokes() {
        // 8.d: prose INSIDE the block between two invokes is part of the markup
        // block and is dropped, matching the v1 batch parser.
        let out = parse_chunks(
            &weather_tools(),
            &[
                "I will check both cities. ]<]minimax[>[<tool_call>\n]<]minimax[>[<invoke name=\"get_weather\">\n]<]minimax[>[<location>NYC]<]minimax[>[</location>\n]<]minimax[>[</invoke>",
                "\nThen check LA.\n]<]minimax[>[<invoke name=\"get_weather\">\n]<]minimax[>[<location>LA]<]minimax[>[</location>\n]<]minimax[>[</invoke>\n]<]minimax[>[</tool_call>",
            ],
        );
        assert_eq!(out.normal_text, "I will check both cities. ");
        let merged = out.coalesce_calls();
        assert_eq!(merged.calls.len(), 2);
        assert_eq!(merged.calls[0].arguments, r#"{"location":"NYC"}"#);
        assert_eq!(merged.calls[1].arguments, r#"{"location":"LA"}"#);
    }

    #[test]
    fn recovers_bare_invoke_without_wrapper() {
        // 5.g-shaped: prose + bare invoke (no `<tool_call>` opener) + orphan close.
        let out = parse_chunks(
            &weather_tools(),
            &[
                "I will check that. ]<]minimax[>[<invoke name=\"get_weather\">\n]<]minimax[>[<location>NYC]<]minimax[>[</location>\n]<]minimax[>[</invoke>",
                "\n]<]minimax[>[</tool_call>",
            ],
        );
        assert_eq!(out.normal_text, "I will check that. ");
        let merged = out.coalesce_calls();
        assert_eq!(merged.calls.len(), 1);
        assert_eq!(merged.calls[0].arguments, r#"{"location":"NYC"}"#);
    }

    #[test]
    fn holds_back_split_orphan_close_after_bare_invoke() {
        // 5.g-shaped, but the orphan `BLOCK_END` close is split across two chunk
        // boundaries while `suppress_normal_text` is latched by the bare-invoke
        // recovery. The split close must be held back whole so the orphan-close
        // path can consume it and clear the latch; otherwise its remainder is
        // unrecognizable, suppression stays on, and the trailing prose is dropped.
        let out = parse_chunks(
            &weather_tools(),
            &[
                "I will check that. ]<]minimax[>[<invoke name=\"get_weather\">\n]<]minimax[>[<location>NYC]<]minimax[>[</location>\n]<]minimax[>[</invoke>\n]<]minimax",
                "[>[</tool",
                "_call>Here is the weather.",
            ],
        );
        // The close marker never leaks, and the trailing prose survives.
        assert_eq!(out.normal_text, "I will check that. Here is the weather.");
        assert!(!out.normal_text.contains("tool_call"));
        assert!(!out.normal_text.contains("minimax"));
        let merged = out.coalesce_calls();
        assert_eq!(merged.calls.len(), 1);
        assert_eq!(merged.calls[0].arguments, r#"{"location":"NYC"}"#);
    }

    #[test]
    fn suppresses_incomplete_invoke_at_eof() {
        // 5.c: block start + invoke truncated mid-value never leaks.
        let out = parse_chunks(
            &weather_tools(),
            &[
                "]<]minimax[>[<tool_call>\n]<]minimax[>[<invoke name=\"get_weather\">",
                "\n]<]minimax[>[<location>NY",
            ],
        );
        assert_eq!(out.normal_text, "");
        assert!(out.calls.iter().all(|call| !call.complete));
        assert!(out.coalesce_calls().calls.is_empty());
    }

    #[test]
    fn holds_back_split_namespace_token() {
        // A chunk boundary inside the namespace token must not leak its prefix.
        let out = parse_chunks(
            &weather_tools(),
            &[
                "text before ]<]mini",
                "max[>[<tool_call>\n]<]minimax[>[<invoke name=\"get_weather\">\n]<]minimax[>[<location>NYC]<]minimax[>[</location>\n]<]minimax[>[</invoke>\n]<]minimax[>[</tool_call>",
            ],
        );
        assert_eq!(out.normal_text, "text before ");
        assert_eq!(out.coalesce_calls().calls.len(), 1);
    }

    #[test]
    fn preserves_source_parameter_order_and_types() {
        // 7.a-shaped: multiple typed parameters keep model-emitted order.
        let tools = vec![Tool {
            name: "book_flight".to_string(),
            description: None,
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "destination": { "type": "string" },
                    "passengers": { "type": "integer" },
                    "first_class": { "type": "boolean" }
                }
            }),
            strict: None,
        }];
        let out = parse_chunks(
            &tools,
            &[
                "]<]minimax[>[<tool_call>\n]<]minimax[>[<invoke name=\"book_flight\">",
                "\n]<]minimax[>[<destination>Paris]<]minimax[>[</destination>",
                "\n]<]minimax[>[<passengers>2]<]minimax[>[</passengers>",
                "\n]<]minimax[>[<first_class>true]<]minimax[>[</first_class>",
                "\n]<]minimax[>[</invoke>\n]<]minimax[>[</tool_call>",
            ],
        );
        let merged = out.coalesce_calls();
        assert_eq!(merged.calls.len(), 1);
        assert_eq!(
            merged.calls[0].arguments,
            r#"{"destination":"Paris","passengers":2,"first_class":true}"#
        );
    }

    #[test]
    fn nested_value_tags_do_not_shadow_top_level_order() {
        // 7.d-shaped: nested array/object tags inside a value must not be taken
        // for top-level parameter names when re-ordering.
        let tools = vec![Tool {
            name: "process_data".to_string(),
            description: None,
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "items": { "type": "array", "items": { "type": "integer" } },
                    "config": { "type": "object" }
                }
            }),
            strict: None,
        }];
        let out = parse_chunks(
            &tools,
            &[
                "]<]minimax[>[<tool_call>\n]<]minimax[>[<invoke name=\"process_data\">",
                "\n]<]minimax[>[<items>]<]minimax[>[<item>1]<]minimax[>[</item>]<]minimax[>[<item>2]<]minimax[>[</item>]<]minimax[>[</items>",
                "\n]<]minimax[>[<config>]<]minimax[>[<mode>fast]<]minimax[>[</mode>]<]minimax[>[</config>",
                "\n]<]minimax[>[</invoke>\n]<]minimax[>[</tool_call>",
            ],
        );
        let merged = out.coalesce_calls();
        assert_eq!(merged.calls.len(), 1);
        let args: serde_json::Value = serde_json::from_str(&merged.calls[0].arguments).unwrap();
        assert_eq!(args["items"], serde_json::json!([1, 2]));
        assert_eq!(args["config"]["mode"], "fast");
        // Top-level order: items before config.
        assert!(
            merged.calls[0].arguments.find("\"items\"").unwrap()
                < merged.calls[0].arguments.find("\"config\"").unwrap()
        );
    }

    #[test]
    fn nested_union_child_values_follow_payload_shape() {
        let tools = vec![Tool {
            name: "list_notes".into(),
            description: None,
            parameters: serde_json::json!({
                "properties": {"pagination": {"anyOf": [
                    {"type": "object", "properties": {
                        "page": {"type": "integer"},
                        "mode": {"anyOf": [{"type": "string"}, {"type": "object"}]},
                        "after": {"type": ["object", "null"]},
                        "config": {"type": "object", "properties": {"enabled": {"type": "boolean"}}}
                    }},
                    {"type": "null"}
                ]}}
            }),
            strict: Some(true),
        }];
        let input = concat!(
            "]<]minimax[>[<tool_call>",
            "]<]minimax[>[<invoke name=\"list_notes\">",
            "]<]minimax[>[<pagination>",
            "]<]minimax[>[<page>2]<]minimax[>[</page>",
            "]<]minimax[>[<mode>one]<]minimax[>[</mode>",
            "]<]minimax[>[<after>null]<]minimax[>[</after>",
            "]<]minimax[>[<config>]<]minimax[>[<enabled>true]<]minimax[>[</enabled>]<]minimax[>[</config>",
            "]<]minimax[>[</pagination>",
            "]<]minimax[>[</invoke>",
            "]<]minimax[>[</tool_call>"
        );
        for width in [1, input.len()] {
            let chunks: Vec<_> = input
                .as_bytes()
                .chunks(width)
                .map(|chunk| std::str::from_utf8(chunk).unwrap())
                .collect();
            let merged = parse_chunks(&tools, &chunks).coalesce_calls();
            assert_eq!(merged.calls.len(), 1);
            let args: serde_json::Value = serde_json::from_str(&merged.calls[0].arguments).unwrap();
            assert_eq!(
                args,
                serde_json::json!({"pagination": {"page": 2, "mode": "one", "after": null, "config": {"enabled": true}}}),
                "width {width}"
            );
        }
    }

    #[test]
    fn literal_null_branch_does_not_erase_nested_integer_type() {
        for union in ["anyOf", "oneOf"] {
            for alternative in [
                serde_json::json!({"enum":[null]}),
                serde_json::json!({"const":null}),
            ] {
                let tools = vec![Tool {
                    name: "list_notes".into(),
                    description: None,
                    strict: None,
                    parameters: serde_json::json!({"properties":{"pagination":{union:[
                        {"type":"object","properties":{"page":{"type":"integer"}}},alternative
                    ]}}}),
                }];
                let input = concat!(
                    "]<]minimax[>[<tool_call>]<]minimax[>[<invoke name=\"list_notes\">",
                    "]<]minimax[>[<pagination>]<]minimax[>[<page>2]<]minimax[>[</page>",
                    "]<]minimax[>[</pagination>]<]minimax[>[</invoke>]<]minimax[>[</tool_call>"
                );
                for width in [1, input.len()] {
                    let chunks: Vec<_> = input
                        .as_bytes()
                        .chunks(width)
                        .map(|c| std::str::from_utf8(c).unwrap())
                        .collect();
                    let out = parse_chunks(&tools, &chunks).coalesce_calls();
                    assert_eq!(out.calls.len(), 1);
                    assert_eq!(
                        serde_json::from_str::<serde_json::Value>(&out.calls[0].arguments).unwrap(),
                        serde_json::json!({"pagination":{"page":2}}),
                        "{union}, {alternative}, width {width}"
                    );
                }
            }
        }
    }

    #[test]
    fn nested_and_encoded_refs_preserve_types_across_chunk_boundaries() {
        let tools = vec![Tool {
            name: "create_order".into(),
            description: None,
            strict: None,
            parameters: serde_json::json!({
                "$defs": {
                    "postal code": {"type": "integer"},
                    "Address": {"type": "object", "properties": {
                        "zip": {"$ref": "#/$defs/postal%20code"},
                        "primary": {"$ref": "#/$defs/Flag"}
                    }},
                    "Flag": {"type": "boolean"}
                },
                "properties": {"shipping": {"$ref": "#/$defs/Address"}}
            }),
        }];
        let input = concat!(
            "Preparing. ]<]minimax[>[<tool_call>]<]minimax[>[<invoke name=\"create_order\">",
            "]<]minimax[>[<shipping>]<]minimax[>[<zip>18956]<]minimax[>[</zip>",
            "]<]minimax[>[<primary>true]<]minimax[>[</primary>]<]minimax[>[</shipping>",
            "]<]minimax[>[</invoke>]<]minimax[>[</tool_call> Done."
        );
        for width in [1, 7, input.len()] {
            let chunks: Vec<_> = input
                .as_bytes()
                .chunks(width)
                .map(|chunk| std::str::from_utf8(chunk).unwrap())
                .collect();
            let out = parse_chunks(&tools, &chunks).coalesce_calls();
            assert_eq!(out.normal_text, "Preparing.  Done.");
            assert_eq!(out.calls.len(), 1);
            assert_eq!(out.calls[0].name.as_deref(), Some("create_order"));
            assert!(out.calls[0].complete);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&out.calls[0].arguments).unwrap(),
                serde_json::json!({"shipping": {"zip": 18956, "primary": true}}),
                "width {width}"
            );
        }
    }
}
