#!/usr/bin/env bash
# Wrapper: inject SSE flags for libwebp-sys SIMD sources under clang-cl.
exec /opt/homebrew/opt/llvm/bin/clang-cl -mssse3 -msse4.1 "$@"
