// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

use dynamo_parsers_v2::{
    Tool, UnifiedEvent, UnifiedParserEvent, UnifiedParserExt, UnifiedParserInit,
    UnifiedParserStartingState, assemble, canonical_unified_family,
    create_unified_parser_for_family,
};

fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "echo".into(),
        description: None,
        parameters: serde_json::json!({
            "type": "object", "properties": {"text": {"type": "string"}},
            "required": ["text"]
        }),
        strict: None,
    }]
}

// Template at nvidia/NVIDIA-Nemotron-3.5-Super-VL-120B-A12B-NVFP4,
// revision 579ffa0b659bc681b89e649fa7c2d5089399602e, opens <think> by default.
#[test]
fn nemotron_aliases_use_the_shared_grammar_and_request_prefill() {
    let call = "<tool_call><function=echo><parameter=text>001</parameter></function></tool_call>";
    for alias in ["nemotron3", "nemotron_v3"] {
        assert_eq!(canonical_unified_family(alias), Some("qwen3"));
        let mut parser = create_unified_parser_for_family(alias, &tools()).unwrap();
        for (state, prefix, reasoning) in [
            (
                UnifiedParserStartingState::Reasoning,
                "look up</think>",
                true,
            ),
            // Nemotron can enter the tool block without closing reasoning.
            (UnifiedParserStartingState::Reasoning, "look up", true),
            (UnifiedParserStartingState::Response, "", false),
        ] {
            let input = format!("{prefix}{call}");
            for split in 0..=input.len() {
                parser.reset();
                parser
                    .initialize_request(UnifiedParserInit {
                        starting_state: state,
                        ..Default::default()
                    })
                    .unwrap();
                let mut events = parser.push(&input[..split]).unwrap();
                events.extend(parser.push(&input[split..]).unwrap());
                events.extend(parser.finish().unwrap().events);
                let mut expected = Vec::new();
                if reasoning {
                    expected.push(UnifiedEvent::Reasoning {
                        text: "look up".into(),
                    });
                }
                expected.push(UnifiedEvent::ToolCall {
                    name: "echo".into(),
                    arguments: serde_json::json!({"text":"001"}),
                });
                assert_eq!(
                    assemble(&events),
                    expected,
                    "{alias} {state:?} split={split}"
                );
            }
        }
    }
}

#[test]
fn nemotron_streams_long_string_arguments_before_call_closure() {
    let mut parser = create_unified_parser_for_family("nemotron_v3", &tools()).unwrap();
    parser
        .initialize_request(UnifiedParserInit {
            starting_state: UnifiedParserStartingState::Reasoning,
            ..Default::default()
        })
        .unwrap();
    let mut events = parser
        .push("consider<tool_call><function=echo><parameter=text>")
        .unwrap();
    assert!(
        events.iter().any(|event| matches!(event,
            UnifiedParserEvent::ToolCall(call) if call.name.as_deref() == Some("echo")
        )),
        "name must be emitted at its closing delimiter"
    );
    let fragment = "x".repeat(1024);
    for _ in 0..128 {
        let deltas = parser.push(&fragment).unwrap();
        assert!(deltas.iter().any(|event| matches!(event,
            UnifiedParserEvent::ToolCall(call) if call.arguments.contains("xxxx") && !call.complete
        )), "argument progress must precede value and call closure");
        events.extend(deltas);
    }
    events.extend(parser.push("</parameter></function></tool_call>").unwrap());
    events.extend(parser.finish().unwrap().events);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event,
                UnifiedParserEvent::ToolCall(call) if call.complete
            ))
            .count(),
        1
    );
    assert_eq!(
        assemble(&events),
        vec![
            UnifiedEvent::Reasoning {
                text: "consider".into()
            },
            UnifiedEvent::ToolCall {
                name: "echo".into(),
                arguments: serde_json::json!({"text":fragment.repeat(128)})
            }
        ]
    );
}
