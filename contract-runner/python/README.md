# Open Contracts Python runner

This package reads a v1 bundle from JSON or YAML, or accepts a consumer reader
that returns the same normalized mapping. Bundles can put cases at the top level
or group them under targets. Each case has `id`, `input`, `expected`, and optional
string-expression `predicates`.

The runner invokes an executor for every target case and records its observation
as `actual`. It evaluates each predicate against that observation first; any
passing predicate accepts the case. If none passes, it compares `actual` exactly
with `expected`. Reports include every target and case. `CoverageAggregator`
pools outcomes by case count across compound runs and enforces a percentage
threshold on the combined result.

The default registry provides `command` and `hooks` targets, each launched as a
separate process per case with `input` JSON on stdin, and generic `http` targets
that POST case input and normalize status and response body. Projects can register
their own target executors and implement `BundleReader` for native manifests;
provider-specific configuration stays outside this package.

## Development

Install the bootstrap predicate package from this repository, then this package:

```sh
python3 -m pip install -e ../../bootstrap-contracts/predicates/python
python3 -m pip install -e '.[dev]'
python3 -m pytest
```

The v1 JSON Schema is `schemas/bundle.v1.schema.json`. Its predicate field
references the standalone string language schema from the bootstrap predicate
package.
