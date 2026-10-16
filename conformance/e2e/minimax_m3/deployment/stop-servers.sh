#!/usr/bin/env bash
set -euo pipefail
pids=()
for file in /weights/frontend-v1.pid /weights/frontend-v2.pid /weights/server.pid; do
  if [ -f "$file" ]; then
    pid=$(cat "$file")
    if [[ "$pid" =~ ^[0-9]+$ ]]; then
      pids+=("$pid")
      kill -TERM -- "-$pid" 2>/dev/null || true
    fi
  fi
done
sleep 10
for pid in "${pids[@]}"; do kill -KILL -- "-$pid" 2>/dev/null || true; done
# This directory contains only this experiment's ephemeral discovery registrations.
rm -rf /weights/discovery
