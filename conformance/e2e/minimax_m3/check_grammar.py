#!/usr/bin/env python3
# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

"""Check production M3 constraints with XGrammar, independently of generation."""
import argparse
import html
import json
from pathlib import Path
import subprocess

import xgrammar
import yaml

NS = "]<]minimax[>["


def native_calls(calls):
    invokes = []
    for call in calls:
        parameters = []
        for key, value in call["arguments"].items():
            text = value if isinstance(value, str) else json.dumps(value)
            parameters.append(f"{NS}<{key}>{html.escape(text, quote=False)}{NS}</{key}>")
        invokes.append(f'{NS}<invoke name="{call["name"]}">\n' + "\n".join(parameters) + f"\n{NS}</invoke>")
    return f"{NS}<tool_call>\n" + "\n".join(invokes) + f"\n{NS}</tool_call>"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag-builder", type=Path, required=True)
    parser.add_argument("--cases", type=Path, default=Path(__file__).with_name("cases.yaml"))
    args = parser.parse_args()
    suite = yaml.safe_load(args.cases.read_text())
    tokenizer = xgrammar.TokenizerInfo([bytes([i]) for i in range(256)] + [b"<eos>"], stop_token_ids=[256])
    compiler = xgrammar.GrammarCompiler(tokenizer, max_threads=1)
    checked = 0
    for case in suite["cases"]:
        if case["id"] not in {"required_integer", "parallel_distinct", "parallel_same_tool", "single_call_policy"}:
            continue
        request = {"tools": case["tools"], "tool_choice": case["tool_choice"],
                   "parallel_tool_calls": case.get("parallel_tool_calls"),
                   "chat_template_kwargs": {"thinking_mode": "disabled"}}
        tag = subprocess.check_output([str(args.tag_builder.resolve())], input=json.dumps(request).encode()).decode()
        compiled = compiler.compile_structural_tag(tag)
        calls = case["expected"]["calls"]
        checks = [("native", native_calls(calls), True)]
        if case["id"] == "required_integer":
            checks += [("empty", native_calls([]), False),
                       ("wrong_integer", native_calls(calls).replace("<a>17", "<a>bad"), False),
                       ("whitespace_loop", native_calls(calls).replace(f"\n{NS}</invoke>", " " * 128 + f"\n{NS}</invoke>"), False)]
        if case["id"] == "parallel_same_tool":
            checks.append(("string_whitespace", native_calls(calls).replace("Paris", "New    York"), True))
        if case["id"] == "single_call_policy":
            checks.append(("parallel_forbidden", native_calls(calls + calls), False))
        for label, text, expected in checks:
            matcher = xgrammar.GrammarMatcher(compiled)
            accepted = matcher.accept_string(text) and matcher.accept_token(256)
            assert accepted == expected, (case["id"], label, accepted, expected)
            print(f'{case["id"]}/{label}: PASS', flush=True)
            checked += 1
    assert checked == 9, f"missing grammar checks: {checked}/9"
    print(f"M3 production grammar: {checked}/9 passed")


if __name__ == "__main__":
    main()
