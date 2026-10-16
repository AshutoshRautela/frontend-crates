#!/usr/bin/env python3
# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
"""Serialize cold MSA cache compilation across tensor-parallel processes."""
from pathlib import Path

path = Path("/opt/MSA/python/fmha_sm100/jit.py")
source = path.read_text()
if "def _process_compile_lock():" not in source:
    source = source.replace("import itertools\n", "import itertools\nimport fcntl\nfrom contextlib import contextmanager\n", 1)
    anchor = "CACHE_BASE = _compute_cache_base()\n"
    assert source.count(anchor) == 1, "unrecognized pinned MSA loader"
    source = source.replace(anchor, anchor + '''

@contextmanager
def _process_compile_lock():
    # Thread locks cannot protect shared .so files across TP processes.
    CACHE_BASE.mkdir(parents=True, exist_ok=True)
    with (CACHE_BASE / ".compile.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield
''', 1)
    for lock in ("self._lock", "_plan_lock", "_sparse_topk_lock", "_reduction_lock"):
        old = f"with {lock}:"
        assert source.count(old) == 1, f"unrecognized MSA lock: {lock}"
        source = source.replace(old, f"with {lock}, _process_compile_lock():", 1)
    compile(source, str(path), "exec")
    path.write_text(source)
print("MSA process-safe cold cache ready")
