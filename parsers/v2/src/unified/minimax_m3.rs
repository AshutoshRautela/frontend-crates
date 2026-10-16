// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! MiniMax M3 reasoning, visible text and namespaced recursive XML tool calls.

use super::{GuidedRouted, ScannerUnified, UnifiedParser};
use crate::Tool;
use crate::tool_calling::minimax_m3::minimax_m3_scanner;
use crate::tool_calling::scan::ReasoningSpec;

pub(crate) fn minimax_m3_unified(tools: &[Tool]) -> Box<dyn UnifiedParser> {
    Box::new(GuidedRouted::new(ScannerUnified::new(
        minimax_m3_scanner(tools).with_reasoning(ReasoningSpec {
            start: "<mm:think>",
            end: "</mm:think>",
            forced_start: false,
            preserve_special_tokens: true,
            ..Default::default()
        }),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unified::{
        UnifiedEvent, UnifiedParserEvent, UnifiedParserExt, UnifiedParserInit,
        UnifiedParserStartingState, assemble,
    };
    use serde_json::json;

    fn tools() -> Vec<Tool> {
        vec![Tool {
            name: "echo".into(),
            description: None,
            strict: None,
            parameters: json!({"type":"object","properties":{"text":{"type":"string"}}}),
        }]
    }

    #[test]
    fn strings_stream_before_parameter_close_with_split_entities_and_namespace() {
        for text_schema in [
            json!({"type":"string"}),
            json!({"$ref":"#/$defs/Text"}),
            json!({"allOf":[{"$ref":"#/$defs/Text"},{"minLength":1}]}),
        ] {
            let mut schemas = tools();
            schemas[0].parameters["$defs"] = json!({"Text":{"type":"string"}});
            schemas[0].parameters["properties"]["text"] = text_schema;
            let mut parser = minimax_m3_unified(&schemas);
            let mut events = Vec::new();
            for chunk in [
                "]<]minimax[>[<tool_call>",
                "]<]minimax[>[<invoke name=\"echo\">",
            ] {
                events.extend(parser.push(chunk).unwrap());
            }
            assert!(
                matches!(&events[..], [UnifiedParserEvent::ToolCall(c)] if c.name.as_deref() == Some("echo") && !c.complete)
            );
            events.extend(parser.push("]<]minimax[>[<text>hello &am").unwrap());
            assert!(
                matches!(events.last(), Some(UnifiedParserEvent::ToolCall(c)) if c.arguments == "{\"text\":\"hello " && !c.complete)
            );
            events.extend(parser.push("p; 世界 ]<]mini").unwrap());
            assert!(
                matches!(events.last(), Some(UnifiedParserEvent::ToolCall(c)) if c.arguments == "& 世界 " && !c.complete)
            );
            events.extend(
                parser
                    .push("max[>[</text>]<]minimax[>[</invoke>]<]minimax[>[</tool_call>")
                    .unwrap(),
            );
            events.extend(parser.finish().unwrap().events);
            assert_eq!(
                assemble(&events),
                vec![UnifiedEvent::ToolCall {
                    name: "echo".into(),
                    arguments: json!({"text":"hello & 世界 "})
                }]
            );
        }
    }

    #[test]
    fn every_character_split_preserves_order_and_reset() {
        let input = "<mm:think>first</mm:think>]<]minimax[>[<tool_call>]<]minimax[>[<invoke name=\"echo\">]<]minimax[>[<text>世界 &lt;tag&gt;]<]minimax[>[</text>]<]minimax[>[</invoke>]<]minimax[>[</tool_call><mm:think>second</mm:think>answer";
        let expected = vec![
            UnifiedEvent::Reasoning {
                text: "first".into(),
            },
            UnifiedEvent::ToolCall {
                name: "echo".into(),
                arguments: json!({"text":"世界 <tag>"}),
            },
            UnifiedEvent::Reasoning {
                text: "second".into(),
            },
            UnifiedEvent::Text {
                text: "answer".into(),
            },
        ];
        for at in input.char_indices().map(|(at, _)| at).chain([input.len()]) {
            let mut parser = minimax_m3_unified(&tools());
            for _ in 0..2 {
                parser
                    .initialize_request(UnifiedParserInit::default())
                    .unwrap();
                let mut events = parser.push(&input[..at]).unwrap();
                events.extend(parser.push(&input[at..]).unwrap());
                events.extend(parser.finish().unwrap().events);
                assert_eq!(assemble(&events), expected, "split {at}");
                parser.reset();
            }
        }
    }

    #[test]
    fn typed_parameter_does_not_block_following_string_progress() {
        let mut parser = minimax_m3_unified(&[Tool {
            name: "echo".into(),
            description: None,
            strict: None,
            parameters: json!({"type":"object","properties":{"count":{"type":"integer"},"text":{"type":"string"}}}),
        }]);
        let mut events = parser.push("]<]minimax[>[<invoke name=\"echo\">]<]minimax[>[<count>42]<]minimax[>[</count>]<]minimax[>[<text>hello").unwrap();
        assert!(events.iter().any(|event| matches!(event, UnifiedParserEvent::ToolCall(call) if call.arguments.contains("42") && call.arguments.contains("hello") && !call.complete)));
        events.extend(
            parser
                .push(" world]<]minimax[>[</text>]<]minimax[>[</invoke>")
                .unwrap(),
        );
        events.extend(parser.finish().unwrap().events);
        assert_eq!(
            assemble(&events),
            vec![UnifiedEvent::ToolCall {
                name: "echo".into(),
                arguments: json!({"count":42,"text":"hello world"})
            }]
        );
    }

    #[test]
    fn repeated_array_parameters_keep_the_legacy_collection() {
        let mut schemas = tools();
        schemas[0].parameters["properties"]["text"] =
            json!({"type":"array","items":{"type":"string"}});
        let mut parser = minimax_m3_unified(&schemas);
        let mut events = parser
            .push("]<]minimax[>[<invoke name=\"echo\">]<]minimax[>[<text>a]<]minimax[>[</text>")
            .unwrap();
        events.extend(
            parser
                .push("]<]minimax[>[<text>b]<]minimax[>[</text>]<]minimax[>[</invoke>")
                .unwrap(),
        );
        events.extend(parser.finish().unwrap().events);
        assert_eq!(
            assemble(&events),
            vec![UnifiedEvent::ToolCall {
                name: "echo".into(),
                arguments: json!({"text":["a","b"]}),
            }]
        );
    }

    #[test]
    fn prefilled_reasoning_and_response_are_request_scoped() {
        for (state, input, expected) in [
            (
                UnifiedParserStartingState::Reasoning,
                "first</mm:think>answer",
                vec![
                    UnifiedEvent::Reasoning {
                        text: "first".into(),
                    },
                    UnifiedEvent::Text {
                        text: "answer".into(),
                    },
                ],
            ),
            (
                UnifiedParserStartingState::Response,
                "literal <mm:think>tag</mm:think>",
                vec![UnifiedEvent::Text {
                    text: "literal <mm:think>tag</mm:think>".into(),
                }],
            ),
        ] {
            let mut parser = minimax_m3_unified(&[]);
            parser
                .initialize_request(UnifiedParserInit {
                    starting_state: state,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(parser.parse_complete(input).unwrap(), expected);
        }
    }
    #[test]
    fn empty_name_cannot_contaminate_the_next_streamed_call() {
        let mut parser = minimax_m3_unified(&tools());
        let mut events = parser
            .push("]<]minimax[>[<tool_call>]<]minimax[>[<invoke name=\"\">")
            .unwrap();
        assert!(events.is_empty());
        events.extend(parser.push("]<]minimax[>[</invoke>]<]minimax[>[<invoke name=\"echo\">]<]minimax[>[<text>ok").unwrap());
        events.extend(
            parser
                .push("]<]minimax[>[</text>]<]minimax[>[</invoke>]<]minimax[>[</tool_call>")
                .unwrap(),
        );
        events.extend(parser.finish().unwrap().events);
        assert_eq!(
            assemble(&events),
            vec![UnifiedEvent::ToolCall {
                name: "echo".into(),
                arguments: json!({"text":"ok"})
            }]
        );
    }
}
