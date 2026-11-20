#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if ! command -v cargo-ndk >/dev/null 2>&1; then
  cargo install cargo-ndk --locked
fi

: "${ANDROID_NDK_HOME:=${ANDROID_NDK_ROOT:-}}"
if [[ -z "$ANDROID_NDK_HOME" ]]; then
  echo "Set ANDROID_NDK_HOME (or ANDROID_NDK_ROOT) to your Android NDK directory." >&2
  exit 1
fi
export ANDROID_NDK_HOME

cargo ndk \
  -t arm64-v8a \
  -t x86_64 \
  -o android/src/main/jniLibs \
  build --release -p navaq-core
