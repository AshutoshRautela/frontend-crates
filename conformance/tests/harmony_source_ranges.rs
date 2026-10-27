// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! GPT-OSS source ranges: every Text and Reasoning byte that the Harmony parser
//! emits maps to the input bytes it came from.
//!
//! The parser is fed decoded text, as dynamo-llm feeds it. Harmony encodes that
//! text again with its own tokenizer and reads content back one token at a time,
//! so the ranges it records are checked against the input, not trusted:
//!
//! * Same output: recording on and off give identical events, errors, and
//!   `reset` text, at every split.
//! * Verbatim and tiling: the ranges taken after each call, read from the input,
//!   equal that call's Text and Reasoning bytes in output order, with no gap.
//! * Causality: no range reaches past the bytes fed so far.
//! * Order: ranges from parse and finish calls never go back in the input.
//!   Byte equality alone accepts a range one repeat off ("ha ha ha"); this does not.
//!
//! The corpus is every GPT-OSS conformance case, the beam fixtures (Harmony
//! syntax), dynamo's GPT-OSS streams and test literals, and targeted cases for
//! the paths that replay or make up tokens. Each stream runs at its recorded
//! split, one character per chunk, and every two-way split at a character
//! boundary. It runs under the fixture's init and under the init dynamo-llm
//! uses, which replays `<|start|>assistant` as prompt tokens.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Mutex;

use dynamo_parsers_v2::unified::harmony::encode_harmony;
use dynamo_parsers_v2::{
    UnifiedParserEvent, UnifiedParserInit, UnifiedParserOutput, UnifiedParserStartingState,
    UnifiedToolOutputMode, create_unified_parser_for_family,
};

const GPT_OSS_FIXTURE: &str = "fixtures-unified-v2/families/gpt_oss/inputs_and_golden.yaml";
const BEAM_GOLDENS: &str = "fixtures-beam/reference-goldens.jsonl";
const BEAM_MESSAGES: &str = "fixtures-beam/original_parser_fixtures.yaml";
const DYNAMO_STREAMS: &str = "tests/data/harmony_dynamo_streams.json";
const HARMONY_MARKERS: &[&str] = &[
    "<|start|>",
    "<|channel|>",
    "<|constrain|>",
    "<|message|>",
    "<|end|>",
    "<|return|>",
    "<|call|>",
];

/// Cases for the paths that replay or make up tokens. Bodies avoid repeated
/// n-grams and mix 1 to 4 byte characters, so a misplaced range cannot match.
const TARGETED: &[(&str, &str)] = &[
    (
        "distinct_codepoints",
        "<|channel|>analysis<|message|>αβγ😀δεζ🌆ηθ<|end|><|start|>assistant<|channel|>final<|message|>ab¢€𐍈cd😀ef<|return|>",
    ),
    (
        "outside_text_before_header",
        "Hello there ✓ <|channel|>final<|message|>X<|return|>",
    ),
    (
        "implicit_recipient_then_text",
        "  to=functions.f<|channel|>commentary<|message|>{}<|call|>after ✓",
    ),
    ("unterminated_final_header", "<|channel|>final ¡olé! 🌆"),
    (
        "unterminated_final_header_with_start",
        "<|start|>assistant<|channel|>final tail ✓",
    ),
    (
        "quoted_control_recovery",
        "<|start|>assistant<|channel|>commentary to=functions.f<|message|>{\"a\":\"x<|end|><|start|>assistant<|channel|>final<|message|>visible ζ€😀",
    ),
    ("orphan_controls", "abc<|end|>déf<|message|>ghi😀"),
    (
        "outside_after_turn_end",
        "<|channel|>final<|message|>Aé<|return|>trailing ✓",
    ),
    (
        "held_marker_prefix_at_eof",
        "<|channel|>final<|message|>abc<|",
    ),
    (
        "unlisted_special_token",
        "<|channel|>final<|message|>x<|endoftext|>y<|return|>",
    ),
    (
        "resync_at_start",
        "<|channel|>final<|message|>aé<|start|>assistant<|channel|>final<|message|>b😀",
    ),
    (
        "repeated_ngrams",
        "<|channel|>analysis<|message|>ha ha ha ha<|end|><|start|>assistant<|channel|>final<|message|>ha ha ha<|return|>",
    ),
];

#[derive(Clone)]
struct Stream {
    id: String,
    chunks: Vec<String>,
    starting_state: UnifiedParserStartingState,
}

impl Stream {
    fn text(&self) -> String {
        self.chunks.concat()
    }
}

fn conformance_path(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(conformance_path(rel)).unwrap_or_else(|error| panic!("{rel}: {error}"))
}

fn starting_state(value: &serde_yaml::Value) -> UnifiedParserStartingState {
    match value.as_str() {
        Some("Reasoning") => UnifiedParserStartingState::Reasoning,
        Some("Response") => UnifiedParserStartingState::Response,
        Some("None") | None => UnifiedParserStartingState::None,
        Some(other) => panic!("unknown starting_state {other}"),
    }
}

/// GPT-OSS cases. Guided cases are returned apart: recording refuses them.
fn gpt_oss_cases() -> (Vec<Stream>, Vec<(String, UnifiedParserInit)>) {
    let document: serde_yaml::Value = serde_yaml::from_str(&read(GPT_OSS_FIXTURE)).unwrap();
    let mut native = Vec::new();
    let mut guided = Vec::new();
    for (name, case) in document["cases"].as_mapping().unwrap() {
        let name = name.as_str().unwrap().to_string();
        let request = &case["request"];
        let init = &request["init"];
        let state = starting_state(&init["starting_state"]);
        let chunks: Vec<String> = request["chunks"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|chunk| chunk["delta_text"].as_str().unwrap().to_string())
            .collect();
        match init["tool_output_mode"].as_str() {
            Some("Native") => native.push(Stream {
                id: format!("gpt_oss/{name}"),
                chunks,
                starting_state: state,
            }),
            _ => {
                let named_tool = init["tool_output_mode"]["GuidedJson"]["named_tool"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| init["named_tool"].as_str().map(str::to_string));
                guided.push((
                    format!("gpt_oss/{name}"),
                    UnifiedParserInit {
                        starting_state: state,
                        tool_output_mode: UnifiedToolOutputMode::GuidedJson { named_tool },
                        ..UnifiedParserInit::default()
                    },
                ))
            }
        }
    }
    (native, guided)
}

fn has_harmony_marker(text: &str) -> bool {
    HARMONY_MARKERS.iter().any(|marker| text.contains(marker))
}

/// Beam fixtures use Harmony syntax with another tool grammar. Fed to GPT-OSS
/// they are adversarial streams: every byte the parser emits must still map back.
fn beam_streams() -> Vec<Stream> {
    let mut streams = Vec::new();
    for line in read(BEAM_GOLDENS)
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        let chunks: Vec<String> = record["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|chunk| chunk.as_str().unwrap().to_string())
            .collect();
        if has_harmony_marker(&chunks.concat()) {
            streams.push(Stream {
                id: format!("beam/{}", record["id"].as_str().unwrap()),
                chunks,
                starting_state: UnifiedParserStartingState::None,
            });
        }
    }
    let document: serde_yaml::Value = serde_yaml::from_str(&read(BEAM_MESSAGES)).unwrap();
    for case in document["message_cases"].as_sequence().unwrap() {
        let text = case["input"]["text"].as_str().unwrap().to_string();
        if has_harmony_marker(&text) {
            streams.push(Stream {
                id: format!("beam_message/{}", case["case_id"].as_str().unwrap()),
                chunks: vec![text],
                starting_state: UnifiedParserStartingState::None,
            });
        }
    }
    streams
}

/// dynamo's recorded GPT-OSS backend streams and Harmony test literals, copied by
/// `tests/data/harmony_dynamo_streams.json` with the dynamo commit it names.
fn dynamo_streams() -> Vec<Stream> {
    let document: serde_json::Value = serde_json::from_str(&read(DYNAMO_STREAMS)).unwrap();
    document["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stream| Stream {
            id: format!("dynamo/{}", stream["id"].as_str().unwrap()),
            chunks: stream["chunks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|chunk| chunk.as_str().unwrap().to_string())
                .collect(),
            starting_state: UnifiedParserStartingState::None,
        })
        .collect()
}

fn targeted_streams() -> Vec<Stream> {
    TARGETED
        .iter()
        .map(|(name, text)| Stream {
            id: format!("targeted/{name}"),
            chunks: vec![text.to_string()],
            starting_state: UnifiedParserStartingState::None,
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
enum InitMode {
    /// The fixture's init, with no prompt tokens.
    Fixture,
    /// dynamo-llm's streaming init: a native request with no prefilled channel
    /// replays `<|start|>assistant` as prompt tokens.
    DynamoLlm,
}

fn init_for(stream: &Stream, mode: InitMode) -> UnifiedParserInit {
    let prompt_token_ids = match (mode, stream.starting_state) {
        (InitMode::DynamoLlm, UnifiedParserStartingState::None) => {
            encode_harmony("<|start|>assistant").unwrap()
        }
        _ => Vec::new(),
    };
    UnifiedParserInit {
        prompt_token_ids,
        starting_state: stream.starting_state,
        tool_output_mode: UnifiedToolOutputMode::Native,
        ..UnifiedParserInit::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    Finish,
    FinishAtEos,
    Reset,
}

/// What one parser call returned, compared between recording on and off.
#[derive(Debug, PartialEq)]
enum CallResult {
    Events(Vec<UnifiedParserEvent>),
    /// Events committed before the error stay committed.
    Error(String, Vec<UnifiedParserEvent>),
    Reset(String),
}

struct Run {
    calls: Vec<CallResult>,
    /// Input bytes fed before each call returned.
    fed: Vec<usize>,
    /// Ranges taken after each call. Empty when not recording.
    ranges: Vec<Vec<Range<usize>>>,
}

fn events_or_error(result: anyhow::Result<()>, output: UnifiedParserOutput) -> CallResult {
    match result {
        Ok(()) => CallResult::Events(output.events),
        Err(error) => CallResult::Error(error.to_string(), output.events),
    }
}

fn run(chunks: &[&str], init: &UnifiedParserInit, end: End, record: bool) -> Run {
    let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
    parser.initialize_request(init.clone()).unwrap();
    if record {
        assert!(parser.record_sources(), "GPT-OSS native refused to record");
    }
    let mut out = Run {
        calls: Vec::new(),
        fed: Vec::new(),
        ranges: Vec::new(),
    };
    let mut fed = 0;
    let mut failed = false;
    for chunk in chunks {
        let mut output = UnifiedParserOutput::default();
        let result = parser.parse_into(chunk, &mut output);
        failed = result.is_err();
        fed += chunk.len();
        out.calls.push(events_or_error(result, output));
        out.fed.push(fed);
        let mut ranges = Vec::new();
        parser.take_sources(&mut ranges);
        out.ranges.push(ranges);
        if failed {
            break;
        }
    }
    // After a parse error the caller recovers held text with `reset`.
    let last = match (end, failed) {
        (End::Reset, _) | (_, true) => CallResult::Reset(parser.reset()),
        (End::Finish, false) => match parser.finish() {
            Ok(output) => CallResult::Events(output.events),
            Err(error) => CallResult::Error(error.to_string(), Vec::new()),
        },
        (End::FinishAtEos, false) => match parser.finish_at_eos() {
            Ok(output) => CallResult::Events(output.events),
            Err(error) => CallResult::Error(error.to_string(), Vec::new()),
        },
    };
    out.calls.push(last);
    out.fed.push(fed);
    let mut ranges = Vec::new();
    parser.take_sources(&mut ranges);
    out.ranges.push(ranges);
    out
}

/// The Text and Reasoning bytes one call committed, in output order.
fn committed_text(call: &CallResult) -> String {
    match call {
        CallResult::Events(events) | CallResult::Error(_, events) => events
            .iter()
            .filter_map(|event| match event {
                UnifiedParserEvent::Text(text) | UnifiedParserEvent::Reasoning(text) => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect(),
        CallResult::Reset(text) => text.clone(),
    }
}

#[derive(Default)]
struct Tally {
    runs: usize,
    text_bytes: usize,
    finish_text_runs: usize,
    reset_text_runs: usize,
    failures: Vec<String>,
}

impl Tally {
    fn fail(&mut self, failure: String) {
        if self.failures.len() < 40 {
            self.failures.push(failure);
        }
    }
}

/// Run one split with recording on and off, and check every rule.
fn check(
    stream: &Stream,
    mode: InitMode,
    label: &str,
    chunks: &[&str],
    end: End,
    tally: &mut Tally,
) {
    let init = init_for(stream, mode);
    let input = chunks.concat();
    let off = run(chunks, &init, end, false);
    let on = run(chunks, &init, end, true);
    tally.runs += 1;
    let at = format!("{} {mode:?} {label} {end:?}", stream.id);
    check_runs(&at, &input, chunks, &off, &on, tally);
}

fn check_runs(at: &str, input: &str, chunks: &[&str], off: &Run, on: &Run, tally: &mut Tally) {
    if on.calls != off.calls {
        tally.fail(format!(
            "{at}: recording changed the output\n  off: {:?}\n  on:  {:?}",
            off.calls, on.calls
        ));
        return;
    }
    let mut floor = 0;
    for (index, call) in on.calls.iter().enumerate() {
        let text = committed_text(call);
        let ranges = &on.ranges[index];
        tally.text_bytes += text.len();
        let is_last = index + 1 == on.calls.len();
        if is_last && !text.is_empty() {
            match call {
                CallResult::Reset(_) => tally.reset_text_runs += 1,
                _ => tally.finish_text_runs += 1,
            }
        }
        let fed = on.fed[index];
        let mut rebuilt = Vec::new();
        for range in ranges {
            if range.end > fed || range.start > range.end {
                tally.fail(format!(
                    "{at}: call {index} range {range:?} is past the {fed} bytes fed\n  input: {input:?}"
                ));
                return;
            }
            if !matches!(call, CallResult::Reset(_)) {
                if range.start < floor {
                    tally.fail(format!(
                        "{at}: call {index} range {range:?} goes back before {floor}\n  ranges: {ranges:?}\n  input: {input:?}"
                    ));
                    return;
                }
                floor = range.end;
            }
            rebuilt.extend_from_slice(&input.as_bytes()[range.clone()]);
        }
        if rebuilt != text.as_bytes() {
            tally.fail(format!(
                "{at}: call {index} ranges do not rebuild the committed text\n  text:    {text:?}\n  rebuilt: {:?}\n  ranges: {ranges:?}\n  input: {input:?}\n  chunks: {chunks:?}",
                String::from_utf8_lossy(&rebuilt)
            ));
            return;
        }
    }
}

fn char_boundaries(text: &str) -> Vec<usize> {
    let mut at: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    at.push(text.len());
    at
}

fn check_stream(stream: &Stream, tally: &mut Tally) {
    let text = stream.text();
    let boundaries = char_boundaries(&text);
    for mode in [InitMode::Fixture, InitMode::DynamoLlm] {
        let recorded: Vec<&str> = stream.chunks.iter().map(String::as_str).collect();
        for end in [End::Finish, End::FinishAtEos, End::Reset] {
            check(stream, mode, "recorded", &recorded, end, tally);
        }
        let chars: Vec<&str> = boundaries
            .windows(2)
            .map(|pair| &text[pair[0]..pair[1]])
            .collect();
        check(stream, mode, "one_char", &chars, End::Finish, tally);
        check(stream, mode, "one_char", &chars, End::FinishAtEos, tally);
        for &split in &boundaries {
            let (head, tail) = text.split_at(split);
            let label = format!("split@{split}");
            check(stream, mode, &label, &[head, tail], End::Finish, tally);
            // Reset after the head recovers held text, which must map back too.
            check(stream, mode, &label, &[head], End::Reset, tally);
        }
    }
}

/// Counts tracing `why` codes, to show which recovery paths the corpus reached.
struct WhyCounter {
    counts: Mutex<BTreeMap<String, usize>>,
}

struct WhyVisitor<'a>(&'a mut Option<String>);

impl tracing::field::Visit for WhyVisitor<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "why" {
            *self.0 = Some(value.to_string());
        }
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "why" {
            *self.0 = Some(format!("{value:?}").trim_matches('"').to_string());
        }
    }
}

impl tracing::Subscriber for WhyCounter {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut why = None;
        event.record(&mut WhyVisitor(&mut why));
        if let Some(why) = why {
            *self.counts.lock().unwrap().entry(why).or_default() += 1;
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn check_corpus(name: &str, streams: Vec<Stream>) -> (Tally, BTreeMap<String, usize>) {
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16);
    let streams = Mutex::new(streams.into_iter());
    let tally = Mutex::new(Tally::default());
    let counter = std::sync::Arc::new(WhyCounter {
        counts: Mutex::new(BTreeMap::new()),
    });
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let counter = counter.clone();
            let (streams, tally) = (&streams, &tally);
            scope.spawn(move || {
                tracing::subscriber::with_default(counter, || {
                    loop {
                        let Some(stream) = streams.lock().unwrap().next() else {
                            return;
                        };
                        let mut mine = Tally::default();
                        check_stream(&stream, &mut mine);
                        let mut all = tally.lock().unwrap();
                        all.runs += mine.runs;
                        all.text_bytes += mine.text_bytes;
                        all.finish_text_runs += mine.finish_text_runs;
                        all.reset_text_runs += mine.reset_text_runs;
                        for failure in mine.failures {
                            all.fail(failure);
                        }
                    }
                })
            });
        }
    });
    let tally = tally.into_inner().unwrap();
    let counts = std::mem::take(&mut *counter.counts.lock().unwrap());
    eprintln!(
        "{name}: {} runs per recording mode, {} committed text bytes, {} runs with finish text, {} with reset text",
        tally.runs, tally.text_bytes, tally.finish_text_runs, tally.reset_text_runs
    );
    eprintln!("{name}: why codes reached (both recording modes): {counts:?}");
    (tally, counts)
}

fn assert_clean(name: &str, tally: &Tally, counts: &BTreeMap<String, usize>) {
    let lost: usize = counts
        .iter()
        .filter(|(why, _)| why.starts_with("harmony_source_"))
        .map(|(_, count)| count)
        .sum();
    assert!(
        tally.failures.is_empty() && lost == 0,
        "{name}: {} failing runs (first {} shown), {lost} unattributed commits\n{}",
        tally.failures.len(),
        tally.failures.len(),
        tally.failures.join("\n")
    );
}

#[test]
fn gpt_oss_cases_map_every_byte_to_its_input() {
    let (native, _) = gpt_oss_cases();
    assert_eq!(native.len(), 225, "the GPT-OSS native corpus changed size");
    let (tally, counts) = check_corpus("gpt_oss", native);
    assert_clean("gpt_oss", &tally, &counts);
}

#[test]
fn beam_streams_map_every_byte_to_its_input() {
    let streams = beam_streams();
    assert!(!streams.is_empty());
    eprintln!("beam: {} streams", streams.len());
    let (tally, counts) = check_corpus("beam", streams);
    assert_clean("beam", &tally, &counts);
}

#[test]
fn dynamo_streams_map_every_byte_to_its_input() {
    let streams = dynamo_streams();
    eprintln!("dynamo: {} streams", streams.len());
    let (tally, counts) = check_corpus("dynamo", streams);
    assert_clean("dynamo", &tally, &counts);
}

#[test]
fn targeted_streams_map_every_byte_to_its_input() {
    let (tally, counts) = check_corpus("targeted", targeted_streams());
    assert_clean("targeted", &tally, &counts);
    // Each replay and recovery path must be reached, or a pass says nothing about it.
    for why in [
        "orphan_harmony_control",
        "malformed_harmony_quoted_control",
        "unterminated_harmony_header",
        "harmony_resync_at_start",
    ] {
        assert!(
            counts.get(why).copied().unwrap_or(0) > 0,
            "no targeted run reached {why}"
        );
    }
    assert!(tally.finish_text_runs > 0 && tally.reset_text_runs > 0);
}

#[test]
fn targeted_paths_emit_the_text_they_test() {
    let init = UnifiedParserInit::default();
    let text_of = |input: &str| {
        let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
        parser.initialize_request(init.clone()).unwrap();
        assert!(parser.record_sources());
        let mut output = UnifiedParserOutput::default();
        parser.parse_into(input, &mut output).unwrap();
        output.append(&mut parser.finish().unwrap());
        let mut ranges = Vec::new();
        parser.take_sources(&mut ranges);
        let text: String = output
            .events
            .iter()
            .filter_map(|event| match event {
                UnifiedParserEvent::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        (text, ranges)
    };
    let cases = [
        (
            "Hello there ✓ <|channel|>final<|message|>X<|return|>",
            "Hello there ✓ X",
        ),
        ("<|channel|>final ¡olé! 🌆", " ¡olé! 🌆"),
        (
            "<|start|>assistant<|channel|>commentary to=functions.f<|message|>{\"a\":\"x<|end|><|start|>assistant<|channel|>final<|message|>visible ζ€😀",
            "visible ζ€😀",
        ),
        (
            "<|channel|>final<|message|>Aé<|return|>trailing ✓",
            "Aétrailing ✓",
        ),
        ("<|channel|>final<|message|>abc<|", "abc<|"),
    ];
    for (input, expected) in cases {
        let (text, ranges) = text_of(input);
        assert_eq!(text, expected, "{input:?}");
        let rebuilt: String = ranges.iter().map(|range| &input[range.clone()]).collect();
        assert_eq!(rebuilt, expected, "{input:?} ranges {ranges:?}");
    }
}

#[test]
fn reset_text_keeps_the_order_it_is_returned_in() {
    // Held whitespace waits in the outside queue, and a later marker prefix waits
    // in the text queue. `reset` returns the text queue first.
    let input = "  <|chan";
    let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
    parser
        .initialize_request(UnifiedParserInit::default())
        .unwrap();
    assert!(parser.record_sources());
    let mut output = UnifiedParserOutput::default();
    parser.parse_into(input, &mut output).unwrap();
    assert!(output.events.is_empty());
    let recovered = parser.reset();
    let mut ranges = Vec::new();
    parser.take_sources(&mut ranges);
    let rebuilt: String = ranges.iter().map(|range| &input[range.clone()]).collect();
    assert_eq!(rebuilt, recovered);
}

#[test]
fn guided_requests_refuse_to_record() {
    let (_, guided) = gpt_oss_cases();
    assert_eq!(guided.len(), 97, "the GPT-OSS guided corpus changed size");
    for (id, init) in guided {
        let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
        parser.initialize_request(init.clone()).unwrap();
        assert!(
            !parser.record_sources(),
            "{id}: guided mode accepted recording"
        );

        let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
        assert!(parser.record_sources());
        assert!(
            parser.initialize_request(init).is_err(),
            "{id}: guided init after recording must fail, not record nothing"
        );
    }
}

#[test]
fn recording_is_refused_after_parsing_starts() {
    let mut parser = create_unified_parser_for_family("gpt_oss", &[]).unwrap();
    parser
        .initialize_request(UnifiedParserInit::default())
        .unwrap();
    parser
        .parse_into(
            "<|channel|>final<|message|>a",
            &mut UnifiedParserOutput::default(),
        )
        .unwrap();
    assert!(!parser.record_sources());
}

#[test]
fn range_checker_rejects_text_sourced_from_the_channel_header() {
    let input = "<|channel|>final<|message|>final";
    let metadata = input.find("final").unwrap();
    let body = input.rfind("final").unwrap();
    assert_ne!(metadata, body);
    let init = UnifiedParserInit::default();
    let mut splits = vec![vec![input]];
    splits.push(
        char_boundaries(input)
            .windows(2)
            .map(|pair| &input[pair[0]..pair[1]])
            .collect(),
    );
    for split in char_boundaries(input) {
        splits.push(vec![&input[..split], &input[split..]]);
    }
    let mut accepted = 0;
    for chunks in &splits {
        let off = run(chunks, &init, End::Finish, false);
        let mut on = run(chunks, &init, End::Finish, true);
        let mut control = Tally::default();
        check_runs("correct sources", input, chunks, &off, &on, &mut control);
        assert!(control.failures.is_empty());
        assert_eq!(
            on.calls.iter().map(committed_text).collect::<String>(),
            "final"
        );
        for range in on.ranges.iter_mut().flatten() {
            assert!(range.start >= body);
            range.start -= body - metadata;
            range.end -= body - metadata;
        }
        let mut tally = Tally::default();
        check_runs("header sources", input, chunks, &off, &on, &mut tally);
        accepted += usize::from(tally.failures.is_empty());
    }
    assert_eq!(
        accepted,
        0,
        "accepted header bytes for {accepted}/{} splits",
        splits.len()
    );
}
