// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

use dynamo_parsers_v2::unified::{
    UnifiedParserExt, UnifiedParserInit, UnifiedToolOutputMode, assemble,
    create_unified_parser_for_family,
};
use serde_json::json;

#[test]
fn explicit_visible_messages_keep_their_phase_and_boundaries() {
    let input = concat!(
        "<|start|>assistant<|channel|>commentary<|message|>Checking. é<|end|>",
        "<|start|>assistant<|channel|>commentary<|message|><|end|>",
        "<|start|>assistant<|channel|>final<|message|>Done.<|return|>"
    );
    let expected = json!([
        {"kind":"message_start", "phase":"commentary"},
        {"kind":"text", "text":"Checking. é"},
        {"kind":"message_end"},
        {"kind":"message_start", "phase":"commentary"},
        {"kind":"message_end"},
        {"kind":"message_start", "phase":"final_answer"},
        {"kind":"text", "text":"Done."},
        {"kind":"message_end"}
    ]);
    for mode in [
        UnifiedToolOutputMode::Native,
        UnifiedToolOutputMode::GuidedJson { named_tool: None },
    ] {
        let guided = !matches!(mode, UnifiedToolOutputMode::Native);
        let input = if guided {
            format!(r#"[{{"name":"lookup","arguments":{{}}}}]{input}"#)
        } else {
            input.to_string()
        };
        let mut expected = expected.clone();
        if guided {
            expected.as_array_mut().unwrap().insert(
                0,
                json!({"kind":"tool_call", "name":"lookup", "arguments":{}}),
            );
        }
        for enabled in [false, true] {
            for split in (0..=input.len()).filter(|i| input.is_char_boundary(*i)) {
                let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
                parser.preserve_message_phases(enabled);
                parser
                    .initialize_request(UnifiedParserInit {
                        tool_output_mode: mode.clone(),
                        ..Default::default()
                    })
                    .unwrap();
                let mut events = parser.push(&input[..split]).unwrap();
                events.extend(parser.push(&input[split..]).unwrap());
                events.extend(parser.finish().unwrap());
                let actual = serde_json::to_value(assemble(&events)).unwrap();
                let wanted = if enabled {
                    expected.clone()
                } else if guided {
                    json!([{"kind":"tool_call", "name":"lookup", "arguments":{}}, {"kind":"text", "text":"Checking. éDone."}])
                } else {
                    json!([{"kind":"text", "text":"Checking. éDone."}])
                };
                assert_eq!(actual, wanted, "{mode:?} enabled={enabled} split={split}");
            }
        }
    }
}

#[test]
fn unfinished_messages_do_not_complete_and_reset_restores_prefilled_phase() {
    use dynamo_parsers_v2::unified::{UnifiedParserEvent, UnifiedParserStartingState};
    let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
    parser.preserve_message_phases(true);
    parser.initialize(&[]).unwrap();
    let mut events = parser
        .push("<|start|>assistant<|channel|>final<|message|>Unfinished")
        .unwrap();
    events.extend(parser.finish().unwrap());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, UnifiedParserEvent::MessageEnd))
    );
    parser.reset();
    parser
        .initialize_request(UnifiedParserInit {
            starting_state: UnifiedParserStartingState::Response,
            ..Default::default()
        })
        .unwrap();
    let mut events = parser.push("Done.<|return|>").unwrap();
    events.extend(parser.finish().unwrap());
    assert_eq!(
        serde_json::to_value(assemble(&events)).unwrap(),
        json!([
            {"kind":"message_start","phase":"final_answer"}, {"kind":"text","text":"Done."}, {"kind":"message_end"}
        ])
    );
}

#[test]
fn empty_prefilled_final_message_is_open_without_a_fabricated_close() {
    use dynamo_parsers_v2::unified::UnifiedParserStartingState;
    let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
    parser
        .initialize_request(UnifiedParserInit {
            starting_state: UnifiedParserStartingState::Response,
            ..Default::default()
        })
        .unwrap();
    parser.preserve_message_phases(true);
    assert_eq!(
        serde_json::to_value(assemble(&parser.finish().unwrap().events)).unwrap(),
        json!([
            {"kind":"message_start","phase":"final_answer"}
        ])
    );
}

#[test]
fn phase_events_follow_original_token_boundaries() {
    use dynamo_parsers_v2::unified::{UnifiedParserOutput, harmony::encode_harmony};
    let tokens = encode_harmony("<|start|>assistant<|channel|>commentary<|message|>é<|end|><|start|>assistant<|channel|>final<|message|>OK<|return|>").unwrap();
    for split in 0..=tokens.len() {
        let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
        parser.initialize(&[]).unwrap();
        parser.preserve_message_phases(true);
        let mut out = UnifiedParserOutput::default();
        parser
            .parse_token_ids_into(&tokens[..split], &mut out)
            .unwrap();
        parser
            .parse_token_ids_into(&tokens[split..], &mut out)
            .unwrap();
        out.events.extend(parser.finish().unwrap());
        assert_eq!(
            serde_json::to_value(out.assembled()).unwrap(),
            json!([
                {"kind":"message_start","phase":"commentary"},{"kind":"text","text":"é"},{"kind":"message_end"},
                {"kind":"message_start","phase":"final_answer"},{"kind":"text","text":"OK"},{"kind":"message_end"}
            ]),
            "token split {split}"
        );
    }
}
