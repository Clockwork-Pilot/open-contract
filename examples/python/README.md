# Python Open Contract examples

This directory contains two Open Contract packages with Python validator
adapters. Both are executed by the single Rust Open Contract runner; their
`contract_command.py` files implement the language batch protocol only.

- [`greeting`](greeting/README.md) is a small validator example.
- [`predicate`](predicate/README.md) exercises a predicate evaluator with a
  472-case dataset.

Run both examples from the Open Contract root:

```sh
make check-python-adapter-examples
```
