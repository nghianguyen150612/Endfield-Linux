#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM
compiler=${CC:-cc}

"$compiler" -std=c17 -Wall -Wextra -Wpedantic -Werror \
    -I"$repo_root/native/optimizer/include" \
    -c "$repo_root/native/optimizer/tests/header_compile_test.c" \
    -o "$build_dir/header_compile_test.o"

"$compiler" -std=c17 -Wall -Wextra -Wpedantic -Werror \
    -I"$repo_root/native/optimizer/include" \
    -c "$repo_root/native/optimizer/src/optimizer.c" \
    -o "$build_dir/optimizer.o"
ar rcs "$build_dir/libendfield_optimizer.a" "$build_dir/optimizer.o"

"$compiler" -std=c17 -Wall -Wextra -Wpedantic -Werror \
    -I"$repo_root/native/optimizer/include" \
    "$repo_root/native/optimizer/tests/optimizer_api_test.c" \
    "$build_dir/libendfield_optimizer.a" \
    -o "$build_dir/optimizer_api_test"

"$build_dir/optimizer_api_test"
