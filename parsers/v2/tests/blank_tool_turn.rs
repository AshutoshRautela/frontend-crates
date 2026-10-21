// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Whitespace-only text in a tool-only turn (issue #406).
//!
//! Nemotron frames each Qwen3-Coder call with a newline on both sides. A turn that
//! only calls tools must not return that framing as text; a turn with any visible
//! text keeps every whitespace byte.

use anyhow::Result;
use dynamo_parsers_v2::{
    Tool, ToolCallDelta, UnifiedEvent, UnifiedParser, UnifiedParserExt, UnifiedParserInit,
    UnifiedParserOutput, UnifiedParserStartingState, assemble, create_unified_parser_for_family,
};

const FAMILIES: [&str; 3] = ["nemotron_v3", "qwen3", "qwen3_coder"];

/// Recorded Nemotron 3.5 Lightning call for "Press digit 1 using the tool."
const DIGIT_CALL: &str = "<tool_call>\n<function=enter_digit>\n<parameter=digit>\n1\n</parameter>\n</function>\n</tool_call>";

/// The recorded tool-only turn, in the chunks the engine delivered.
const RECORDED_CHUNKS: [&str; 4] = [
    "\n",
    "<tool_call>\n<function=enter_digit>\n<parameter=digit>\n",
    "1\n</parameter>\n</function>\n</tool_call>",
    "\n",
];

fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "enter_digit".into(),
        description: None,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"digit": {"type": "string"}},
            "required": ["digit"]
        }),
        strict: None,
    }]
}

fn digit_call(digit: &str) -> String {
    DIGIT_CALL.replace("\n1\n", &format!("\n{digit}\n"))
}

fn call(digit: &str) -> UnifiedEvent {
    UnifiedEvent::ToolCall {
        name: "enter_digit".into(),
        arguments: serde_json::json!({"digit": digit}),
    }
}

fn text(value: &str) -> UnifiedEvent {
    UnifiedEvent::Text { text: value.into() }
}

fn reason(value: &str) -> UnifiedEvent {
    UnifiedEvent::Reasoning { text: value.into() }
}

/// The whole input, every two-way split (with an empty middle chunk), and one
/// chunk per character.
fn chunkings(input: &str) -> Vec<Vec<&str>> {
    let boundaries: Vec<_> = input
        .char_indices()
        .map(|(at, _)| at)
        .chain([input.len()])
        .collect();
    let mut chunks: Vec<_> = boundaries
        .iter()
        .map(|&at| vec![&input[..at], "", &input[at..]])
        .collect();
    chunks.push(boundaries.windows(2).map(|w| &input[w[0]..w[1]]).collect());
    chunks
}

fn run(family: &str, state: UnifiedParserStartingState, chunks: &[&str]) -> Vec<UnifiedEvent> {
    let mut parser = create_unified_parser_for_family(family, &tools()).unwrap();
    parser
        .initialize_request(UnifiedParserInit {
            starting_state: state,
            ..Default::default()
        })
        .unwrap();
    let mut output = UnifiedParserOutput::default();
    for chunk in chunks {
        parser.parse_into(chunk, &mut output).unwrap();
    }
    output.append(&mut parser.finish().unwrap());
    assemble(&output.events)
}

fn assert_every_split(
    family: &str,
    state: UnifiedParserStartingState,
    input: &str,
    expected: &[UnifiedEvent],
) {
    for chunks in chunkings(input) {
        assert_eq!(
            run(family, state, &chunks),
            expected,
            "{family}: {chunks:?}"
        );
    }
}

const RESPONSE: UnifiedParserStartingState = UnifiedParserStartingState::Response;

#[test]
fn recorded_tool_only_turn_emits_no_text() {
    for family in FAMILIES {
        assert_eq!(
            run(family, RESPONSE, &RECORDED_CHUNKS),
            [call("1")],
            "{family}"
        );
        assert_eq!(
            run(family, RESPONSE, &[&RECORDED_CHUNKS.concat()]),
            [call("1")],
            "{family}"
        );
    }
}

#[test]
fn tool_only_turn_emits_no_text_at_every_split() {
    let turn = format!("\n{DIGIT_CALL}\n");
    for family in FAMILIES {
        assert_every_split(family, RESPONSE, &turn, &[call("1")]);
    }
}

#[test]
fn whitespace_between_and_after_calls_is_dropped() {
    let turn = format!("\n{}\n\n{}\n", digit_call("1"), digit_call("2"));
    for family in FAMILIES {
        assert_every_split(family, RESPONSE, &turn, &[call("1"), call("2")]);
    }
    for family in FAMILIES {
        let mixed = format!(" \t\n{DIGIT_CALL}\r\n ");
        assert_eq!(run(family, RESPONSE, &[&mixed]), [call("1")], "{family}");
    }
}

#[test]
fn prose_before_the_call_keeps_every_whitespace_byte() {
    let turn = format!("Pressing it now.\n\n{DIGIT_CALL}\n");
    for family in FAMILIES {
        assert_every_split(
            family,
            RESPONSE,
            &turn,
            &[text("Pressing it now.\n\n"), call("1"), text("\n")],
        );
    }
    let leading = format!("\n\nPressing it now.\n{DIGIT_CALL}");
    for family in FAMILIES {
        assert_every_split(
            family,
            RESPONSE,
            &leading,
            &[text("\n\nPressing it now.\n"), call("1")],
        );
    }
}

/// Held whitespace returns in front of the visible text that decided it, so the
/// text channel is byte-identical; only its position relative to an earlier call
/// moves.
#[test]
fn prose_after_the_call_keeps_every_whitespace_byte() {
    let turn = format!("\n{DIGIT_CALL}\nDone.");
    for family in FAMILIES {
        assert_every_split(family, RESPONSE, &turn, &[call("1"), text("\n\nDone.")]);
    }
}

#[test]
fn whitespace_only_output_without_a_call_is_kept() {
    for family in FAMILIES {
        for input in ["\n\n", " \t\n", "\n"] {
            assert_every_split(family, RESPONSE, input, &[text(input)]);
        }
    }
}

#[test]
fn reasoning_then_a_tool_only_turn_keeps_reasoning_and_drops_framing() {
    let turn = format!("Press it.</think>\n\n{DIGIT_CALL}\n");
    for family in FAMILIES {
        assert_every_split(
            family,
            UnifiedParserStartingState::Reasoning,
            &turn,
            &[reason("Press it."), call("1")],
        );
    }
    let explicit = format!("<think>Press it.</think>\n{DIGIT_CALL}\n");
    assert_every_split(
        "qwen3",
        UnifiedParserStartingState::None,
        &explicit,
        &[reason("Press it."), call("1")],
    );
}

#[test]
fn hold_is_bounded_and_releases_past_the_cap() {
    const CAP: usize = 4096;
    for family in FAMILIES {
        let at_cap = format!("{}{DIGIT_CALL}", "\n".repeat(CAP));
        assert_eq!(run(family, RESPONSE, &[&at_cap]), [call("1")], "{family}");

        let past_cap = "\n".repeat(CAP + 1);
        let turn = format!("{past_cap}{DIGIT_CALL}");
        assert_eq!(
            run(family, RESPONSE, &[&turn]),
            [text(&past_cap), call("1")],
            "{family}"
        );
        // Delivered a byte at a time, holding stops at the byte that crosses the cap
        // and stays off for the rest of the stream.
        let bytes: Vec<String> = past_cap.chars().map(String::from).collect();
        let mut chunks: Vec<&str> = bytes.iter().map(String::as_str).collect();
        chunks.push(DIGIT_CALL);
        chunks.push("\n");
        assert_eq!(
            run(family, RESPONSE, &chunks),
            [text(&past_cap), call("1"), text("\n")],
            "{family}"
        );
    }
}

/// DSML parsers own their call framing (#405): whitespace that is not the
/// encoder's separator stays content, so they are not subject to this rule.
#[test]
fn dsml_families_keep_their_whitespace_contract() {
    for (family, call) in [
        (
            "deepseek_v4",
            "<｜DSML｜tool_calls><｜DSML｜invoke name=\"run\"><｜DSML｜parameter name=\"value\" string=\"true\">Paris</｜DSML｜parameter></｜DSML｜invoke></｜DSML｜tool_calls>",
        ),
        (
            "deepseek_v41",
            "<｜DSML｜ calls><｜DSML｜ invoke name=\"run\"><｜DSML｜ parameter name=\"value\" string=\"true\">Paris</｜DSML｜ parameter></｜DSML｜ invoke></｜DSML｜ calls>",
        ),
    ] {
        let expected_call = UnifiedEvent::ToolCall {
            name: "run".into(),
            arguments: serde_json::json!({"value": "Paris"}),
        };
        let mut parser = create_unified_parser_for_family(family, &[]).unwrap();
        parser
            .initialize_request(UnifiedParserInit {
                starting_state: RESPONSE,
                ..Default::default()
            })
            .unwrap();
        let input = format!("\n\n\n\n{call}");
        let events = parser.parse_complete(&input).unwrap();
        assert_eq!(events, [text("\n\n"), expected_call], "{family}");
    }
}

/// A vendor parser that echoes text, emits a call for `CALL`, and fails on `ERR`.
struct ScriptedVendor {
    calls: usize,
}

impl UnifiedParser for ScriptedVendor {
    fn parse_into(&mut self, delta: &str, output: &mut UnifiedParserOutput) -> Result<()> {
        match delta {
            "ERR" => anyhow::bail!("scripted failure"),
            "CALL" => {
                output.push_call(ToolCallDelta {
                    tool_index: self.calls,
                    name: Some("enter_digit".into()),
                    arguments: "{\"digit\":\"1\"}".into(),
                    complete: true,
                });
                self.calls += 1;
            }
            _ => output.push_text(delta),
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<UnifiedParserOutput> {
        Ok(UnifiedParserOutput::default())
    }
}

fn scripted_vendor(_tools: &[Tool]) -> Result<Box<dyn UnifiedParser>> {
    Ok(Box::new(ScriptedVendor { calls: 0 }))
}

#[test]
fn vendor_parsers_get_the_same_rule_and_errors_keep_committed_text() {
    let family = "blank_tool_turn_vendor";
    dynamo_parsers_v2::register_unified_parser(family, scripted_vendor);

    let mut parser = create_unified_parser_for_family(family, &[]).unwrap();
    let events = parser.parse_complete("\n").unwrap();
    assert_eq!(events, [text("\n")], "whitespace without a call is kept");

    let mut parser = create_unified_parser_for_family(family, &[]).unwrap();
    let mut events = Vec::new();
    for chunk in ["\n", "CALL", "\n"] {
        events.extend(parser.push(chunk).unwrap());
    }
    events.extend(parser.finish().unwrap().events);
    assert_eq!(assemble(&events), [call("1")]);

    // On `Err`, held whitespace was committed by the parser, so it reaches the caller.
    let mut parser = create_unified_parser_for_family(family, &[]).unwrap();
    let mut output = UnifiedParserOutput::default();
    parser.parse_into("\n", &mut output).unwrap();
    assert!(
        output.is_empty(),
        "whitespace is held before the turn decides"
    );
    assert!(parser.parse_into("ERR", &mut output).is_err());
    assert_eq!(assemble(&output.events), [text("\n")]);

    // `reset` starts a fresh stream: nothing held carries over.
    parser
        .parse_into(" ", &mut UnifiedParserOutput::default())
        .unwrap();
    parser.reset();
    let events = parser.parse_complete("Hi").unwrap();
    assert_eq!(events, [text("Hi")]);

    dynamo_parsers_v2::unregister_unified_parser(family);
}
