# Dataset generation bootstrap contract

This standalone package turns a structural schema into deterministic
representative inputs for a function or class interface. An agent derives the
schema from the callable's prototype; the generator does not inspect source
code or depend on a programming language. For a function, the root object can
describe its named arguments. For a class, it can describe constructor and
method-call data using nested objects and arrays. The supported input dialect
is specified in `schema.schema.json`. Generated cases contain only `input`
values, without expected outputs or predicates. The package does not read or
modify consumer manifests.

The supported dialect includes primitive types and type unions, recursive
objects and arrays, tuple arrays through `prefixItems`, `required`, and
user-controlled `enum`, `const`, and `examples`. It also accepts `$schema`,
`$id`, `title`, and `description` annotations. Numeric schemas accept inclusive
`minimum`/`maximum` and exclusive `exclusiveMinimum`/`exclusiveMaximum`
bounds. String schemas accept `minLength`/`maxLength`. The generator samples
numeric endpoints and an interior value, and string length endpoints and an
interior length. `enum` and `const` constrain candidates; `examples` adds
explicit scenarios and must satisfy the schema. When no values or bounds are
specified, type-driven representative defaults are used. Optional object
properties are generated both absent and present. Property names are ordered
lexically so output is stable regardless of YAML key order. Generation is
capped at 512 cases, and generated strings at 4096 characters.
Unsupported schema keywords are rejected so constraints cannot be silently
ignored. An `examples` value that violates its schema is rejected as well.

For example, an agent inspecting `issueLicense(customer, options)` could provide:

```yaml
type: object
required: [customer]
properties:
  customer:
    type: object
    required: [id]
    properties:
      id: {type: string}
  options:
    type: object
    properties:
      trial: {type: boolean}
```

The generated `input` objects fit that argument shape. The schema does not
encode expected results; those remain the responsibility of ordinary tests or
explicit contract dataset cases.

Generate JSON dataset output from a YAML or JSON schema:

```sh
cargo run --quiet --manifest-path runner/bootstrap-contracts/dataset-generation/Cargo.toml \
  --bin generate-dataset -- path/to/data-schema.yaml
```

The package's own `dataset.yaml` exercises schema-to-dataset behavior. Run
only this contract and its required coverage check from the Open Contract
repository root:

```sh
make check-dataset-generation-contract
```
