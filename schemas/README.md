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

## Per-dataset value schemas

Manifest `datasets` entries may remain path strings, or use a mapping to
associate that dataset with schemas for its user-defined values:

```yaml
datasets:
  - path: datasets/create.yaml
    schema: schemas/create.yaml
```

Schema references are package-relative paths (or declared directory aliases)
and must also be readable through `filesystem.read`. Each schema is one YAML
document with top-level `input` and/or `expected` definitions. The Rust runner
applies each definition to every matching value in standalone cases and
sequence steps. A missing `expected` is not validated; the dataset's normal
expected-or-predicate rule still applies. These schemas do not describe or
extend Open Contract's case, step, setup, predicate, parallel, or coverage
fields.

The compact value-schema dialect supports primitive type names (`string`,
`integer`, `number`, `boolean`, `null`) and mappings for objects and arrays.
Object mappings use `type: object`, optional `required`, and a `fields` mapping;
unknown fields are allowed unless `additional: false` is set. Array mappings
use `type: array` and an `items` schema. Any mapping may constrain values with
`enum`; strings support `min_length` and `max_length`, arrays support
`min_items` and `max_items`, and numbers support `minimum` and `maximum`.
Unsupported types, keywords, and malformed schema definitions fail before the
adapter runs. For example:

```yaml
input:
  type: object
  required: [name]
  fields:
    name: string
    tags: {type: array, items: string}
    age: {type: integer, minimum: 0}
expected:
  type: object
  required: [accepted]
  fields: {accepted: boolean}
```

The batch-execution bootstrap contract exercises the runner against
`dataset.v1.schema.json`. The runner compiles that shared schema at build time
and validates JSON and YAML datasets against it before deserializing them.
It then applies any per-dataset YAML value schemas before adapter execution.
