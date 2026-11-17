# Rust contract-language adapter

This crate is linked into a contract-owned Rust executable. It provides
`run_dataset`, which invokes the contract validator once for each flattened
request and returns keyed outputs or errors. The callback receives the real
input and a context object with `case_id`, optional `step_id`, optional `setup`
fixture ID, and `parallel`. Adjacent parallel steps in a case run concurrently;
the other calls run in order. The adapter does not evaluate expected values or
predicates.

With Rust coverage instrumentation enabled, the adapter snapshots LLVM
counters around each validator call, then uses `llvm-profdata` and `llvm-cov`
to return per-request line scopes, aggregate executable and covered lines, and
the native LLVM export. The crate exposes the `OCOVCASE` raw-profile bundle
format used by Rust dataset fingerprint tooling. The Rust runner hashes the
returned case scopes against manifest fingerprints.

Build the contract executable with the same compiler instrumentation flags
used by the bootstrap coverage checker:

```sh
RUSTFLAGS="-Cinstrument-coverage --cfg=coverage --check-cfg=cfg(coverage)" cargo build
```

Install the LLVM tools matching the compiler, then install the adapter's
dependencies from the Open Contract root:

```sh
rustup component add llvm-tools-preview
make -C contract-languages/rust install-dependencies
```

The installation copies `llvm-profdata`, `llvm-cov`, and their LLVM libraries
from the active toolchain into ignored `.deps/bin` and `.deps/lib` directories.
It reuses that installation until the selected compiler changes; `RUSTC` selects
the compiler. `runtime.yaml` declares `.deps` and supplies the tool paths as
`LLVM_PROFDATA` and `LLVM_COV`. Examples invoke this installation goal themselves.
No example-generated isolation manifest or host sysroot access is needed.

When used outside the runner, the adapter also accepts `LLVM_PROFDATA` and
`LLVM_COV`, falling back to tools on `PATH`. They must match the compiler's LLVM
version. The contract must declare a writable directory for short-lived profiles
and set `TMPDIR` accordingly.

The contract-owned command reads the request from stdin and writes one adapter
JSON object to stdout. A minimal entrypoint looks like this:

```rust
use open_contract_language_rust::run_dataset;
use serde_json::{json, Value};
use std::io::{self, Read};

fn validate(input: &Value, _context: &Value) -> Result<Value, String> {
    Ok(json!({"accepted": input["value"].is_number()}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    let request: Value = serde_json::from_str(&text)?;
    let report = run_dataset(&request, validate)
        .map_err(io::Error::other)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
```

See [`examples/rust`](../../examples/rust/README.md) for parent and child Rust
contracts using the adapter.

Run the adapter protocol tests from the `open-contract` directory with:

```sh
make check-rust-language-adapter
```
