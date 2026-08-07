---
name: allez-oneshot
description: MUST USE for any Python invocation — scripts, modules, pip/package installs, REPL, or one-off snippets. Activate whenever running `python`, `python3`, `pip`, `uv` or `pixi` directly would otherwise be considered; use `allez oneshot` instead to run it in an ephemeral, disposable environment.
---

# Use `allez oneshot` for Python

Whenever a task requires running anything involving Python — a script, a module, `pip`/package installs, a REPL, a one-off snippet, or any other Python invocation — run it through `allez oneshot` instead of invoking `python`/`python3`/`pip`/`uv`/`pixi` directly.

`allez oneshot` builds an ephemeral, disposable environment on the fly and runs the given command inside it, so there's no dependency on whatever (if anything) happens to be on the host's `PATH` or already installed globally.

## Usage

```sh
allez oneshot [PACKAGES]... -- <COMMAND> [ARGS...]
```

- Everything before `--` is the list of conda package names to install into the ephemeral environment (space-separated, zero or more)
- Everything after `--` is the pass-through command and its own arguments, run unmodified inside that environment

### Examples

Run a script with the default Python:

```sh
allez oneshot -- python script.py
```

Run a script that needs extra packages:

```sh
allez oneshot numpy pandas -- python script.py
```
