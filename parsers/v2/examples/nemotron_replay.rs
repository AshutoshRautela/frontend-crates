// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Replay saved decoded model chunks without inference; emit exact event positions.
use dynamo_parsers_v2::{
    InvalidGuidedPayloadPolicy, Tool, UnifiedParserEvent, UnifiedParserExt, UnifiedParserInit,
    UnifiedParserStartingState, UnifiedToolOutputMode, assemble, create_unified_parser_for_family,
};
use serde::Deserialize;
use std::io::Read;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Replay {
    tools: Vec<Tool>,
    #[serde(default)]
    starting_state: String,
    #[serde(default)]
    guided_json: bool,
    #[serde(default)]
    named_tool: Option<String>,
    chunks: Vec<String>,
}

fn event(event: &UnifiedParserEvent) -> serde_json::Value {
    match event {
        UnifiedParserEvent::Text(text) => serde_json::json!({"text":text}),
        UnifiedParserEvent::Reasoning(text) => serde_json::json!({"reasoning":text}),
        UnifiedParserEvent::ToolCall(call) => serde_json::json!({"tool_call":{
            "tool_index":call.tool_index,"name":call.name,
            "arguments":call.arguments,"complete":call.complete
        }}),
    }
}

fn main() -> anyhow::Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let replay: Replay = serde_json::from_str(&input)?;
    let mut parser = create_unified_parser_for_family("nemotron_v3", &replay.tools)?;
    let starting_state = match replay.starting_state.as_str() {
        "" | "none" => UnifiedParserStartingState::None,
        "reasoning" => UnifiedParserStartingState::Reasoning,
        "response" => UnifiedParserStartingState::Response,
        other => anyhow::bail!("unknown starting_state {other:?}"),
    };
    anyhow::ensure!(
        replay.guided_json || replay.named_tool.is_none(),
        "named_tool requires guided_json"
    );
    parser.initialize_request(UnifiedParserInit {
        starting_state,
        tool_output_mode: if replay.guided_json {
            UnifiedToolOutputMode::GuidedJson {
                named_tool: replay.named_tool,
            }
        } else {
            UnifiedToolOutputMode::Native
        },
        invalid_guided_payload: InvalidGuidedPayloadPolicy::StreamBestEffort,
        ..Default::default()
    })?;
    let mut events = Vec::new();
    let mut positions = Vec::new();
    let mut bytes = 0;
    for (chunk, text) in replay.chunks.iter().enumerate() {
        bytes += text.len();
        let deltas = parser.push(text)?;
        positions.push(serde_json::json!({"chunk":chunk,"bytes":bytes,"events":deltas.iter().map(event).collect::<Vec<_>>()}));
        events.extend(deltas);
    }
    let finish = parser.finish()?;
    positions.push(serde_json::json!({"finish":true,"bytes":bytes,"events":finish.events.iter().map(event).collect::<Vec<_>>()}));
    events.extend(finish.events);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "positions": positions, "assembled": assemble(&events)
        }))?
    );
    Ok(())
}
