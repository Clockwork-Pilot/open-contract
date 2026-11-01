# Open Contracts Python runner

For native JavaScript execution with Node coverage, see the [JavaScript
runner](../javascript/README.md).

The runner reads a normalized dataset from JSON or YAML, or accepts a custom
reader that returns the same structure. Cases can be listed at the top level or
grouped under targets. Every case has an `id`, an `input`, and exactly one of
`expected` or a complete string-expression `predicate`.

The runner executes once per case and records the observation as `actual`. An
`expected` case passes when `actual` matches it exactly. A `predicate` case
passes when its one complete expression evaluates to true against `actual`.
Reports contain each target and case result. Coverage is opt-in: a caller can
collect it during a run, append data across runs, and choose a threshold to
enforce. Coverage measures custom reader and executor modules automatically;
`--coverage-source` adds other source modules or paths. For runs without custom
Python adapters, provide at least one `--coverage-source` explicitly.

## Execution flow

```text
  JSON/YAML dataset -> built-in reader --+
                                         +--> schema validation -> normalized bundle
  native input -> project reader --------+                              |
                                                                        v
                                                               iterate each case
                                                                        |
                            +-------------------------------------------+----------------+
                            |                                                            |
                            v                                                            v
                 evaluator receives case.input                     expected or predicate
                            |                                                            |
                          actual ---------------------> exact compare / predicate eval --+
                                                                                         |
                                                                                         v
                                                                                case and target report

  optional coverage collection surrounds a run; the caller can combine
  multiple run files and enforce a threshold in coverage-report.
```

For a regular case, the evaluator receives the case's `input` and returns the
observed value. For a sequence case, it receives each resolved step `input` in
order (parallel groups may be concurrent). The runner checks step matchers and
then compares the complete sequence result with the case's `expected` or
`predicate`; the evaluator does not receive either comparison rule. A custom
in-process executor can keep state between its calls. For sequences,
`FunctionExecutor` can accept a second context argument containing `case_id`,
`step_id`, `step_index`, and captures available before that step. Built-in
command targets start a new process for each step, so stateful sequences using
commands need their own persistence mechanism.

For example, collect the two examples into one coverage file and enforce a
caller-selected 80% threshold after both have run:

```sh
PYTHONPATH=contract-runner/python/src:bootstrap-contracts/predicates/python/src \
python3 -m open_contracts_runner run \
  contract-runner/python/examples/raw-dataset/dataset.json \
  --executor evaluator:evaluate --coverage --coverage-file /tmp/open-contracts.coverage
PYTHONPATH=contract-runner/python/src:bootstrap-contracts/predicates/python/src \
python3 -m open_contracts_runner run \
  contract-runner/python/examples/ingested-dataset/input.json \
  --reader ingestor:read_dataset --executor evaluator:evaluate \
  --coverage --coverage-append --coverage-file /tmp/open-contracts.coverage
PYTHONPATH=contract-runner/python/src:bootstrap-contracts/predicates/python/src \
python3 -m open_contracts_runner coverage-report \
  --coverage-file /tmp/open-contracts.coverage --coverage-threshold 80
```

Use `--coverage` without a threshold to collect and report without enforcing a
minimum. Add `--coverage-threshold N` to a run to enforce immediately, or use
`coverage-report` to enforce after accumulating multiple runs.

The built-in registry provides `command` and `hooks` targets, each launched as a
separate process per case (or per sequence step) with input JSON on stdin, and
generic `http` targets that POST case or step input and normalize status and
response body. Projects can register custom executors and provide readers for
native manifests without teaching this package provider-specific formats.

## Focused examples

Each example has its own `evaluator.py`. Its function takes the case input and
returns the observed greeting. Open Contracts performs the expected-value or
predicate comparison. The nested result makes path and existence checks
meaningful in the predicates.

### Raw dataset

`examples/raw-dataset/dataset.json` is already in runner format. Open Contracts
validates it against `schemas/bundle.v1.schema.json`, including the referenced
predicate-language schema, before executing it. It contains
exact-match and predicate cases, for example:

```json
{
  "id": "greets-valid-name-exact",
  "input": { "name": "Ada" },
  "expected": { "greeting": "Hello, Ada!" }
}
```

```json
{
  "id": "greets-grace-compound-predicate",
  "input": {
    "name": "Grace",
    "member": { "active": true, "tier": "silver" }
  },
  "predicate": "exists(.recipient.name) AND .recipient.name == \"Grace\" AND (.recipient.active == true OR .recipient.tier == \"silver\") AND NOT (.recipient.tier == \"blocked\") AND .greeting == \"Hello, Grace!\""
}
```

This single expression demonstrates dotted paths, `exists()`, comparisons,
parentheses, and the `AND`, `OR`, and `NOT` operators.

Run it from the repository root:

```sh
make example-raw-dataset
```

### Ingested dataset

`examples/ingested-dataset/input.json` is source data, not a runner dataset.
`examples/ingested-dataset/ingestor.py` transforms it into normalized case
entries on the fly. For each profile it returns an exact-match case and a
compound-predicate case with the same `input` and either `expected` or
`predicate` shape consumed by the evaluator.

```sh
make example-ingested-dataset
```

`make examples` runs both focused examples with `python -m
open_contracts_runner`. The root Makefile exposes the runner and predicate
source directories to Python, so no package installation or command on `PATH`
is needed. The Make targets request a concise pass count; direct CLI runs print
the full JSON report by default.

For other JSON or YAML source formats, pass a pure adapter with
`--reader MODULE:CALLABLE`; it receives the decoded value and returns the
normalized dataset. `--executor MODULE:CALLABLE` selects an evaluator called
with each case input, or with each resolved step input for a sequence case.

## Ordered sequences

A case may contain `steps` as well as its usual `input` and final matcher. Each
step has an `id` and an `input`; the runner sends that input to the same target
executor, records the returned value under `actual.steps`, and can check the
step with its own `expected` or `predicate`. A step's `capture` maps names to
JSON Pointers in its result. Later inputs can use `{"$capture": "name"}` to
reuse a captured value or `{"$input": "/path"}` to select from the case input.
References substitute JSON values, so numbers, booleans, arrays, and objects
keep their types.

Use a `{parallel: [...]}` item when steps are independent and may run
concurrently. Values captured by a parallel group become available after the
whole group finishes. Step expectations contribute to the case result, and the
case's final matcher can inspect `actual.steps`, `actual.captures`, and
`actual.checks`. The runner only sequences executor calls; each executor still
defines what its input means. Both runners exercise the shared
[`sequences.yaml`](../examples/sequences.yaml) example.

## Predicate language

The bundle schema is `schemas/bundle.v1.schema.json`. Its `predicate` field
references the standalone schema at
`bootstrap-contracts/predicates/predicate-language.schema.json`. That schema
describes the complete string language supported by the bootstrap evaluator:
field paths, string and scalar literals, all comparison and logical operators,
parentheses, `exists()`, precedence, and evaluation rules.

The predicate language schema declares supported operations and data-type
samples. The separate `bootstrap-contracts/predicates/dataset-cases.json` holds
authored cases to exercise; the generator combines those with schema-derived
type/operator combinations. The `generate-dataset` Make goal builds the
complete dataset. Run it through the coverage-driven evaluator sandbox with:

```sh
make -C bootstrap-contracts/predicates check-predicate-contract
```
