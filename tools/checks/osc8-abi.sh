#!/bin/sh
# Requires cargo build --lib; exercises the generated header with a C compiler.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
osc8_core_lib="${PIGEON_CORE_LIB:-$root/core/target/debug}"
"${CC:-cc}" -std=c11 -Wall -Wextra -Werror -I"$root/include" "$here/osc8-abi.c" \
    -L"$osc8_core_lib" -Wl,-rpath,"$osc8_core_lib" -lpigeon_core -o "$work/osc8-abi"
"$work/osc8-abi"
