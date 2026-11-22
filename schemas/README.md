# Open Contracts schemas

`contract-manifest.v1.schema.json` defines the contract manifest.
`language-adapter-runtime.v1.schema.json` defines the flat runtime manifest
referenced by a contract's `language_adapter` field.

The manifest may include an optional top-level `metadata` object for
consumer-owned JSON-compatible values. Open Contract preserves this data on
the parsed manifest and does not assign it execution semantics. Other unknown
top-level manifest fields remain invalid.

This directory contains schemas for two related, but distinct, documents:

- [`bundle.v1.schema.json`](bundle.v1.schema.json) describes a version 1
  Open Contracts bundle. A bundle contains either top-level `cases` or
  `targets`; targets can group cases and carry a `kind` and `config`. Cases
  can be single calls or sequences of `steps`. Bundle steps support fields
  such as `capture` and `description` in addition to inputs and assertions.
- [`dataset.v1.schema.json`](dataset.v1.schema.json) describes the version 1
  dataset document accepted by the runner's `run-manifest` path. It has a
  `version` and `cases`; each case is a call or a sequence of steps. Calls
  and steps support `input`, an `expected` value or `predicate`, an optional
  `setup` fixture name, and an optional `coverage_fingerprint`.

The bundle schema describes the Open Contracts bundle format. The dataset
schema describes the dataset shape the runner loads. They are not
interchangeable: for example, the runner dataset schema does not define bundle
`targets` or step `capture` fields. Both schemas represent `parallel` as a
boolean on an individual step; consecutive steps marked `true` may run
together.

The batch-execution bootstrap contract exercises the runner against
`dataset.v1.schema.json`. The runner compiles that shared schema at build time
and validates JSON and YAML datasets against it before deserializing them.
