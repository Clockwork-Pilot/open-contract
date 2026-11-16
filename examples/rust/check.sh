#!/bin/sh
set -eu

example_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$example_dir/../.." && pwd)
parent="$example_dir/parent"
target_root="$repo_root/.coverage/rust-nested-target"
profile_root="$target_root/profiles"
cargo=${CARGO:-cargo}

cd "$repo_root"
rm -rf "$target_root"
mkdir -p .coverage
trap 'rm -f "$parent/contract-command" "$parent/child/contract-command"' EXIT HUP INT TERM

CARGO_TARGET_DIR="$target_root/parent" \
RUSTFLAGS="-Cinstrument-coverage --cfg=coverage --check-cfg=cfg(coverage)" \
  "$cargo" build --manifest-path "$example_dir/Cargo.toml" \
  -p rust-parent-contract-example
CARGO_TARGET_DIR="$target_root/child" \
RUSTFLAGS="-Cinstrument-coverage --cfg=coverage --check-cfg=cfg(coverage)" \
  "$cargo" build --manifest-path "$example_dir/Cargo.toml" \
  -p rust-child-contract-example

cp "$target_root/parent/debug/contract-command" "$parent/contract-command"
cp "$target_root/child/debug/contract-command" "$parent/child/contract-command"
chmod +x "$parent/contract-command" "$parent/child/contract-command"

rustc=${RUSTC:-rustc}
host=$($rustc -vV | sed -n 's/^host: //p')
llvm_tools="$($rustc --print sysroot)/lib/rustlib/$host/bin"
LLVM_PROFDATA=${LLVM_PROFDATA:-$llvm_tools/llvm-profdata} \
LLVM_COV=${LLVM_COV:-$llvm_tools/llvm-cov} \
LLVM_PROFILE_FILE="$profile_root/%m-%p.profraw" \
PATH="$llvm_tools:$PATH" \
  "$cargo" run --manifest-path runner/rust/Cargo.toml -- run-manifest \
  "$parent/contract.yaml" --isolation none \
  --coverage-report .coverage/rust-nested-report.json
