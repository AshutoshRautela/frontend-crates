#!/usr/bin/env bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev libclang-dev protobuf-compiler libprotobuf-dev cmake git curl unzip patchelf python3-dev python3-venv
export RUSTUP_HOME=/work/rustup CARGO_HOME=/work/cargo
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /work/rustup.sh
sh /work/rustup.sh -y --profile minimal --default-toolchain 1.96.1 --no-modify-path
export PATH="/work/cargo/bin:$PATH"
mkdir -p /work/dynamo /work/frontend
 tar -xf /tmp/dynamo-source.tar.gz -C /work/dynamo
 tar -xf /tmp/frontend-source.tar.gz -C /work/frontend
cd /work/dynamo
patch -p1 < /tmp/dynamo-integration.patch
patch -p1 < /tmp/pinned-engine-compat.patch
patch -p1 < /tmp/dynamo-schema-fallback.patch
cp /tmp/dynamo.Cargo.lock Cargo.lock
export CARGO_TARGET_DIR=/work/target CARGO_BUILD_JOBS=10 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_OPT_LEVEL=1
export PYO3_PYTHON=/usr/bin/python3
cd lib/bindings/python
cargo build --locked --no-default-features
mkdir -p /work/overlay/dynamo
cp -a src/dynamo/. /work/overlay/dynamo/
cp -a /work/dynamo/components/src/dynamo/. /work/overlay/dynamo/
cp /work/target/debug/lib_core.so /work/overlay/dynamo/_core.so
PYTHONPATH=/work/overlay python3 -m dynamo.frontend --help > /work/frontend-help.txt
printf 'BUILD_COMPLETE\n'
