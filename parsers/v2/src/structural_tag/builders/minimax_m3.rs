// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{StructuralTagBuilder, ToolCallGrammar, triggered_calls_format};
use crate::structural_tag::policy::{ResolvedToolCallingPolicy, resolve_tool_schema};
use crate::structural_tag::wire::{
    Format, JsonSchemaFormat, JsonSchemaStyle, TagFormat, TagsWithSeparatorFormat,
};
use serde_json::Value;

pub(crate) static MINIMAX_M3: StructuralTagBuilder = StructuralTagBuilder::new(&MinimaxM3Format);

struct MinimaxM3Format;

const TRIGGER: &str = "]<]minimax[>[<tool_call>";
const INVOKE: &str = "]<]minimax[>[<invoke";

impl MinimaxM3Format {
    fn tool_call_block(
        &self,
        policy: &ResolvedToolCallingPolicy<'_>,
        tool_arguments_any_order: bool,
    ) -> TagFormat {
        // M3 puts all parallel invocations inside one tool_call wrapper.
        let tags = policy
            .tools
            .iter()
            .map(|tool| TagFormat {
                begin: format!("{INVOKE} name=\"{}\">", tool.name),
                content: Box::new(Format::JsonSchema(JsonSchemaFormat {
                    json_schema: resolve_tool_schema(tool, policy.schema_mode),
                    style: JsonSchemaStyle::MinimaxM3Xml,
                    any_order: tool_arguments_any_order,
                    max_whitespace_cnt: Some(2),
                })),
                end: "]<]minimax[>[</invoke>\n".into(),
            })
            .collect();
        TagFormat {
            begin: format!("{TRIGGER}\n"),
            content: Box::new(Format::TagsWithSeparator(TagsWithSeparatorFormat {
                tags,
                separator: String::new(),
                at_least_one: true,
                stop_after_first: policy.stop_after_first(),
            })),
            end: "]<]minimax[>[</tool_call>".into(),
        }
    }
}

impl ToolCallGrammar for MinimaxM3Format {
    fn supports_schema(&self, schema: &Value) -> bool {
        supports_schema(schema)
    }

    fn reasoning_begin(&self) -> Option<&'static str> {
        Some("<mm:think>")
    }

    fn reasoning_end(&self) -> Option<&'static str> {
        Some("</mm:think>")
    }

    fn tool_call_excludes(&self) -> &'static [&'static str] {
        &["]<]minimax[>["]
    }

    fn build_triggered_calls(
        &self,
        policy: &ResolvedToolCallingPolicy<'_>,
        exclude_special_tokens: bool,
        tool_arguments_any_order: bool,
    ) -> anyhow::Result<Format> {
        Ok(triggered_calls_format(
            TRIGGER,
            vec![self.tool_call_block(policy, tool_arguments_any_order)],
            self.reasoning_begin(),
            self.reasoning_end(),
            &[INVOKE],
            exclude_special_tokens,
            policy,
        ))
    }

    fn build_tool_calls_only(
        &self,
        policy: &ResolvedToolCallingPolicy<'_>,
        tool_arguments_any_order: bool,
    ) -> anyhow::Result<Format> {
        Ok(Format::Tag(
            self.tool_call_block(policy, tool_arguments_any_order),
        ))
    }
}

/// M3 XML omits value types, so unions such as string/null are ambiguous.
/// XGrammar also rejects constrained XML strings because their rules cannot
/// be intersected with its namespace-marker exclusion. Preserve those schemas
/// through the caller's JSON-guidance path instead of weakening the constraint.
pub(super) fn supports_schema(schema: &Value) -> bool {
    let mut pending = vec![schema];
    while let Some(schema) = pending.pop() {
        if schema
            .get("type")
            .and_then(Value::as_array)
            .is_some_and(|types| types.len() > 1)
            || ["anyOf", "oneOf"].iter().any(|key| {
                schema
                    .get(key)
                    .and_then(Value::as_array)
                    .is_some_and(|branches| branches.len() > 1)
            })
            || schema
                .get("minLength")
                .is_some_and(|length| length.as_u64() != Some(0))
            || ["maxLength", "pattern", "format"]
                .iter()
                .any(|key| schema.get(key).is_some())
        {
            return false;
        }
        for key in [
            "properties",
            "patternProperties",
            "$defs",
            "definitions",
            "dependentSchemas",
        ] {
            if let Some(children) = schema.get(key).and_then(Value::as_object) {
                pending.extend(children.values());
            }
        }
        for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
            if let Some(children) = schema.get(key).and_then(Value::as_array) {
                pending.extend(children);
            }
        }
        for key in [
            "items",
            "additionalProperties",
            "contains",
            "propertyNames",
            "not",
            "if",
            "then",
            "else",
            "unevaluatedProperties",
        ] {
            if let Some(child) = schema.get(key) {
                if let Some(children) = child.as_array() {
                    pending.extend(children);
                } else {
                    pending.push(child);
                }
            }
        }
    }
    true
}
