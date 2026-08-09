---
name: allez-oneshot
description: MUST USE for any Python invocation — scripts, modules, pip/package installs, REPL, or one-off snippets. Activate whenever running `python`, `python3`, `pip`, `uv` or `pixi` directly would otherwise be considered; use `allez oneshot` instead to run it in an ephemeral, disposable environment.
---

# Use `allez oneshot` for Python

Whenever a task requires running anything involving Python — a script, a module, `pip`/package installs, a REPL, a one-off snippet, or any other Python invocation — run it through `allez oneshot` instead of invoking `python`/`python3`/`pip`/`uv`/`pixi` directly.

`allez oneshot` builds an ephemeral, disposable environment on the fly and runs the given command inside it. Name every package the command needs, `python` included, so the environment supplies them instead of whatever happens to be installed on the host.

## Usage

```sh
allez oneshot [PACKAGES]... -- <COMMAND> [ARGS...]
```

- Everything before `--` is the list of conda package names to install into the ephemeral environment (space-separated, zero or more)
- Everything after `--` is the pass-through command and its own arguments, run unmodified inside that environment

### Default packages

`allez` ships no default package list of its own. With no packages named before `--`, the environment gets exactly whatever the caller's own `~/.condarc` `create_default_packages` setting resolves to.

That setting is usually unset, which is conda's own default for it too. Then the environment is created with no packages at all, not even `python`, and a command run inside it falls through to the host like any other. So `allez oneshot -- python script.py` runs the *host* Python, not an isolated one. Name `python` explicitly unless the caller's `~/.condarc` is known to supply it.

Packages named before `--` are added to that resolved default set, not used instead of it.

A named package whose bare name matches a default-set entry replaces that one entry, version and build constraints included, rather than both ending up installed. Every other default-set entry is unaffected.

### Examples

Run a script, naming the Python it needs:

```sh
allez oneshot python -- python script.py
```

Run a script that needs extra packages:

```sh
allez oneshot python numpy pandas -- python script.py
```
