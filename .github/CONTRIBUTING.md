# Contributing

- One change per pull request. Keep personal preferences, changed defaults included, out of it.
- The shell looks and behaves as the original does. A change to that is a difference: it
  needs a reason and a line in [docs/original-differences.md](../docs/original-differences.md).
- The production binary links no Qt, no KDE and no Quickshell. They are reference tools of
  the tests only.

# Setting up

What the build needs: [docs/build.md](../docs/build.md). Once per clone:

```sh
pre-commit install
```

# Before a pull request

```sh
tools/ci/gate --workspace
```

It needs only Docker. Steps, partial runs and timings: [docs/ci.md](../docs/ci.md).

# Tests

- A change in behaviour comes with a test that fails without it.
- A test compares with the original, Quickshell or Qt where one of them does the same thing.
- A test waits on an event, never on the clock: [docs/testing.md](../docs/testing.md).

# Where things are

[docs/layout.md](../docs/layout.md).
