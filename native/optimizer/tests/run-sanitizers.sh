#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM
compiler=${CC:-cc}

"$compiler" -std=c17 -Wall -Wextra -Wpedantic -Werror \
    -fsanitize=address,undefined -fno-omit-frame-pointer \
    -I"$repo_root/native/optimizer/include" \
    "$repo_root/native/optimizer/src/optimizer.c" \
    "$repo_root/native/optimizer/tests/optimizer_api_test.c" \
    -o "$build_dir/optimizer_api_test_sanitized"

ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=1} \
UBSAN_OPTIONS=${UBSAN_OPTIONS:-print_stacktrace=1:halt_on_error=1} \
    "$build_dir/optimizer_api_test_sanitized"
