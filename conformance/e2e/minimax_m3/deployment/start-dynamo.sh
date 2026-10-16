#!/usr/bin/env bash
set -euo pipefail
interval=${1:?interval}
tags=${2:?true or false}
export PYTHONPATH=/weights/overlay HF_HOME=/weights/huggingface DYN_FILE_KV=/weights/discovery DYN_NAMESPACE=m3-unified
mkdir -p "$DYN_FILE_KV"
nohup setsid python3 -m dynamo.sglang \
  --namespace m3-unified --discovery-backend file \
  --request-plane tcp --response-plane tcp --event-plane zmq \
  --dyn-tool-call-parser minimax_m3 --dyn-reasoning-parser minimax_m3 \
  --dyn-structural-tag "$tags" --reasoning-parser minimax-m3 \
  --model-path MiniMaxAI/MiniMax-M3-MXFP8 \
  --revision c5454eb03678d8710e54a4e0fc681b9f3b4a3dba \
  --served-model-name MiniMaxAI/MiniMax-M3-MXFP8 \
  --tp 4 --attention-backend fa4 --moe-runner-backend deep_gemm \
  --context-length 8192 --chunked-prefill-size 4096 \
  --mem-fraction-static 0.85 --max-running-requests 16 \
  --disable-cuda-graph --stream-interval "$interval" --trust-remote-code \
  > "/weights/dynamo-worker-i${interval}-tags-${tags}.log" 2>&1 < /dev/null &
echo "$!" > /weights/server.pid
for version in 1 2; do
  port=8001
  frontend_overlay=/weights/overlay
  if [ "$version" = 2 ]; then
    port=8000
    frontend_overlay=${M3_FRONTEND_V2_OVERLAY:-/weights/overlay-whitespace}
  fi
  PYTHONPATH="$frontend_overlay" DYN_PARSER_VERSION="$version" nohup setsid python3 -m dynamo.frontend \
    --namespace m3-unified --discovery-backend file \
    --request-plane tcp --response-plane tcp --event-plane zmq \
    --http-port "$port" \
    > "/weights/frontend-v${version}-i${interval}-tags-${tags}.log" 2>&1 < /dev/null &
  echo "$!" > "/weights/frontend-v${version}.pid"
done
