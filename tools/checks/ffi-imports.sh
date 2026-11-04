#!/bin/sh
# A C adapter must reach core semantics through the same interface as Rust clients.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
if rg -n 'crate::(term|session|layout|status|proto)(::|;)' "$root/core/src/ffi"; then
    echo 'FFI must use pigeon_core::api, not internal modules' >&2
    exit 1
fi
echo 'FFI imports: api only'
