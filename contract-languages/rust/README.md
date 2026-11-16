# Rust contract-language adapter

This crate is linked into a contract-owned Rust executable. It provides
`run_dataset`, which invokes the contract validator once for each flattened
request, in order, and returns keyed outputs or errors. The callback receives
the whole request so contract code can inspect its key or step metadata; it
should pass `request["input"]` to the contract's public function. It does not
evaluate expected values or predicates.

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

The adapter looks for `llvm-profdata` and `llvm-cov` on `PATH`. Set
`LLVM_PROFDATA` and `LLVM_COV` when the matching tools are elsewhere. They must
match the LLVM version used by the Rust compiler. The process needs a writable
system temporary directory for short-lived per-case profiles.

The contract-owned command reads the request from stdin and writes one adapter
JSON object to stdout. A minimal entrypoint looks like this:

```rust
use open_contract_language_rust::run_dataset;
use serde_json::{json, Value};
use std::io::{self, Read};

fn validate(input: &Value) -> Result<Value, String> {
    Ok(json!({"accepted": input["value"].is_number()}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    let request: Value = serde_json::from_str(&text)?;
    let report = run_dataset(&request, |item| validate(&item["input"]))
        .map_err(io::Error::other)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
```

See [`examples/nested`](examples/nested/README.md) for parent and child Rust
contracts using the adapter.

Run the adapter protocol tests from the `open-contract` directory with:

```sh
make check-rust-language-adapter
```
