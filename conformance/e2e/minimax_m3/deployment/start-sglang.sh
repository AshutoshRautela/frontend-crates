#!/usr/bin/env bash
set -euo pipefail
interval=${1:-1}
nohup setsid python3 -m sglang.launch_server \
  --model-path MiniMaxAI/MiniMax-M3-MXFP8 \
  --revision c5454eb03678d8710e54a4e0fc681b9f3b4a3dba \
  --served-model-name MiniMaxAI/MiniMax-M3-MXFP8 \
  --reasoning-parser minimax-m3 --tool-call-parser minimax-m3 \
  --tp 4 --attention-backend fa4 --moe-runner-backend deep_gemm \
  --context-length 8192 --chunked-prefill-size 4096 \
  --mem-fraction-static 0.85 --max-running-requests 16 \
  --disable-cuda-graph --stream-interval "$interval" \
  --host 0.0.0.0 --port 8000 --trust-remote-code \
  > "/weights/sglang-interval-${interval}.log" 2>&1 < /dev/null &
echo "$!" > /weights/server.pid
cat /weights/server.pid
