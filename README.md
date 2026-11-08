# Open Contract

Open Contract provides dataset-driven tools for testing portable API contracts.
The repository includes Python and JavaScript contract runners and bootstrap
predicate contracts.

## Bubblewrap helper

`bin/bwrap-run` is a runner-agnostic process wrapper. It adds fail-closed Bubblewrap
startup flags and passes every Bubblewrap option and the child command through
unchanged. The caller declares runtime and filesystem mounts before `--`, then
supplies any command after it. The helper does not inspect datasets, select a
language, or infer dependencies.

```sh
sh open-contract/bin/bwrap-run \
  --ro-bind /usr /usr \
  --ro-bind /usr/lib /usr/lib \
  --tmpfs /tmp \
  --setenv PATH /usr/bin:/bin \
  -- /usr/bin/sh
```

Bubblewrap must be installed and support the requested namespaces. Missing
Bubblewrap or a namespace failure returns an error; the helper never retries the
command outside the sandbox. `--isolation none -- <command> [args...]` is the
explicit trusted-debug opt-out.

Applications should keep their mount policy in a shell script and pass their
existing runner command as the opaque child command. The license-server repository
contains one such policy for its contract datasets.
