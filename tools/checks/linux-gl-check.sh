#!/bin/sh
# Resource and buffer validation on a private software GL display.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/config" "$work/cache" "$work/runtime"
chmod 700 "$work/runtime"
cd "$root"
# Leave HOME alone so rustup/cargo can find the already-installed offline toolchain.
GDK_BACKEND=x11 GTK_A11Y=none GIO_USE_VFS=local LIBGL_ALWAYS_SOFTWARE=1 \
XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache" XDG_RUNTIME_DIR="$work/runtime" \
xvfb-run -a -s '-screen 0 640x480x24 -nolisten tcp' \
    cargo test -p pigeon-linux --locked --offline \
    render::gl::tests::resources_are_released_on_their_owner_and_uploads_are_checked \
    -- --exact --ignored --nocapture
