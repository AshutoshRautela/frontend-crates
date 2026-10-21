// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Whitespace-only text in a tool-only turn (issue #406).
//!
//! Nemotron frames each Qwen3-Coder call with a newline on both sides, so a turn
//! that only calls tools parsed to `Text("\n") | call | Text("\n")`. The rule here
//! is the one the v1 jail applies (`hold_blank_content` / `settle_blank_content` in
//! `dynamo-parsers`), so both crates answer the same turn the same way:
//!
//! - Whitespace-only text is held until visible (non-whitespace) text arrives. It is
//!   then released unchanged in front of that text, and holding stops for the rest
//!   of the stream.
//! - At finish, held whitespace is dropped if the stream produced a tool call and no
//!   visible text; otherwise it is released unchanged.
//! - Holding stops at [`MAX_HELD_BLANK_BYTES`], so a model stuck emitting newlines
//!   still streams.
//!
//! Reasoning and tool-call events pass through untouched and in order. The visible
//! text channel is byte-identical whenever the turn has visible text; the one thing
//! that can move is where held whitespace sits relative to an earlier reasoning or
//! call event, because it is released in front of the visible text that decided it.
//!
//! # Why a wrapper in the factory, not a scanner change
//!
//! The rule is about the turn, not a grammar: whether whitespace is framing depends
//! on bytes that have not arrived yet (a later call, or later prose), which no single
//! family's scanner position can see. Applying it once around whatever parser the
//! factory builds gives every built-in and vendor family the same answer from one
//! implementation, the same way the v1 jail applies it to every tool parser.
//!
//! DeepSeek DSML is the exception. Its parsers already own their call framing
//! (#405): the encoder renders `content + "\n\n" + calls`, the parser consumes
//! exactly that separator, and any other whitespace is content that must survive so
//! re-rendering reproduces the generated tokens. Dropping it would break that
//! contract, so those families are not wrapped (see [`owns_call_framing`]).

use super::{UnifiedParser, UnifiedParserEvent, UnifiedParserInit, UnifiedParserOutput};
use crate::tool_calling::traits::Result;

/// Whitespace held before visible text is released past this size. Matches the v1 jail.
pub(crate) const MAX_HELD_BLANK_BYTES: usize = 4096;

/// Families whose parser defines its own call framing and keeps the remaining
/// whitespace as content. Keyed by canonical family name.
pub(crate) fn owns_call_framing(canonical_family: &str) -> bool {
    matches!(canonical_family, "deepseek_v4" | "deepseek_v41")
}

/// Wrap `inner` with the tool-only-turn whitespace rule unless its family owns its
/// call framing.
pub(crate) fn wrap(
    canonical_family: &str,
    inner: Box<dyn UnifiedParser>,
) -> Box<dyn UnifiedParser> {
    if owns_call_framing(canonical_family) {
        return inner;
    }
    Box::new(BlankToolTurn::new(inner))
}

/// Holds whitespace-only text until the turn shows whether it is framing.
struct BlankToolTurn {
    inner: Box<dyn UnifiedParser>,
    /// Whitespace-only text not yet released.
    held: String,
    /// Holding is over for this stream: visible text arrived or the cap was reached.
    closed: bool,
    /// This stream produced a tool-call event.
    made_tool_call: bool,
}

impl BlankToolTurn {
    fn new(inner: Box<dyn UnifiedParser>) -> Self {
        Self {
            inner,
            held: String::new(),
            closed: false,
            made_tool_call: false,
        }
    }

    /// Route one advance's events into `output`, holding whitespace-only text.
    fn filter(&mut self, events: UnifiedParserOutput, output: &mut UnifiedParserOutput) {
        for event in events {
            match event {
                UnifiedParserEvent::Text(text) if !self.closed => {
                    if text.trim().is_empty()
                        && self.held.len() + text.len() <= MAX_HELD_BLANK_BYTES
                    {
                        self.held.push_str(&text);
                        continue;
                    }
                    self.closed = true;
                    output.push_text(std::mem::take(&mut self.held));
                    output.push_text(text);
                }
                UnifiedParserEvent::ToolCall(call) => {
                    self.made_tool_call = true;
                    output.push_call(call);
                }
                other => output.push_event(other),
            }
        }
    }

    /// Release held whitespace unchanged and stop holding.
    fn release(&mut self, output: &mut UnifiedParserOutput) {
        self.closed = true;
        output.push_text(std::mem::take(&mut self.held));
    }
}

impl UnifiedParser for BlankToolTurn {
    fn initialize_request(&mut self, init: UnifiedParserInit) -> Result<()> {
        self.inner.initialize_request(init)
    }

    fn parse_into(&mut self, delta: &str, output: &mut UnifiedParserOutput) -> Result<()> {
        let mut advanced = UnifiedParserOutput::default();
        let result = self.inner.parse_into(delta, &mut advanced);
        self.filter(advanced, output);
        if result.is_err() {
            // The error contract keeps committed output with the caller. Held
            // whitespace was committed by the inner parser, so it goes out too.
            self.release(output);
        }
        result
    }

    fn finish(&mut self) -> Result<UnifiedParserOutput> {
        let tail = self.inner.finish()?;
        let mut filtered = UnifiedParserOutput::default();
        self.filter(tail, &mut filtered);
        if self.held.is_empty() {
            return Ok(filtered);
        }
        if self.made_tool_call {
            self.held.clear();
            self.closed = true;
            return Ok(filtered);
        }
        // Held whitespace precedes everything this finish produced.
        let mut output = UnifiedParserOutput::default();
        self.release(&mut output);
        output.append(&mut filtered);
        Ok(output)
    }

    fn reset(&mut self) -> String {
        // The held whitespace belongs to the abandoned stream, like its emitted output.
        self.held.clear();
        self.closed = false;
        self.made_tool_call = false;
        self.inner.reset()
    }

    fn preserve_special_tokens(&self) -> bool {
        self.inner.preserve_special_tokens()
    }

    fn tool_call_id(&self, tool_index: usize) -> Option<&str> {
        self.inner.tool_call_id(tool_index)
    }
}
