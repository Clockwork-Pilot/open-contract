#!/bin/sh
set -eu

cd "$(dirname "$0")"
rustc=${RUSTC:-rustc}
sysroot=$($rustc --print sysroot)
host=$($rustc -vV | sed -n 's/^host: //p')
tools="$sysroot/lib/rustlib/$host"
identity=$(printf '%s\n%s\n' "$sysroot" "$($rustc -vV)")
if [ -f .deps/toolchain ] && [ "$(cat .deps/toolchain)" = "$identity" ]; then
    exit 0
fi
if [ ! -f "$tools/bin/llvm-profdata" ] || [ ! -f "$tools/bin/llvm-cov" ]; then
    echo 'Install the matching Rust LLVM tools: rustup component add llvm-tools-preview' >&2
    exit 1
fi

rm -rf .deps
mkdir -p .deps/bin .deps/lib
cp "$tools/bin/llvm-profdata" "$tools/bin/llvm-cov" .deps/bin/
# Preserve the tools' ../lib lookup layout; do not grant access to the sysroot.
for library in "$tools/lib"/libLLVM*; do
    [ -f "$library" ] || continue
    cp -L "$library" .deps/lib/
done
printf '%s\n' "$identity" > .deps/toolchain
