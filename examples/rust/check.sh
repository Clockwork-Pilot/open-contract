#!/bin/sh
set -eu

example_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$example_dir/../.." && pwd)
parent="$example_dir/parent"
target_root="$repo_root/.coverage/rust-nested-target"
profile_root=.open-contract-work
scratch_root=.open-contract-work
cargo=${CARGO:-cargo}
isolation=${OPEN_CONTRACT_ISOLATION:-auto}

cd "$repo_root"
rm -rf "$target_root" .open-contract-work "$parent/$scratch_root" "$parent/child/$scratch_root" "$parent/.deps" "$parent/child/.deps" "$parent/.coverage" "$parent/child/.coverage"
rm -f .coverage/rust-nested-report.json
mkdir -p .coverage "$target_root" .open-contract-work "$parent/$scratch_root" "$parent/child/$scratch_root"
cleanup() {
  rm -f "$parent/contract-command" "$parent/child/contract-command"
  rm -rf "$parent/$scratch_root" "$parent/child/$scratch_root" "$parent/.deps" "$parent/child/.deps" "$parent/.coverage" "$parent/child/.coverage"
  rm -rf "$repo_root/.open-contract-work"
}
trap cleanup EXIT HUP INT TERM

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
cp -R "$repo_root/contract-languages/rust/.deps" "$parent/.deps"
cp -R "$repo_root/contract-languages/rust/.deps" "$parent/child/.deps"
chmod +x "$parent/contract-command" "$parent/child/contract-command"

make -C contract-languages/rust install-dependencies
"$cargo" run --manifest-path runner/rust/Cargo.toml -- run-manifest \
  "$parent/open-contract.yaml" --isolation "$isolation" \
  --coverage-report .coverage/rust-nested-report.json
