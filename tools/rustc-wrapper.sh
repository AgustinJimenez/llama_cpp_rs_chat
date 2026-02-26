#!/usr/bin/env sh
RUSTC="$1"
shift

if command -v sccache >/dev/null 2>&1; then
  exec sccache "$RUSTC" "$@"
fi

exec "$RUSTC" "$@"
