#!/usr/bin/env python3
"""Generate fixtures for `docs/condarc_research.md` §1.3 ("Aliases and
`MultipleKeysError`").

Unlike the boolish-key generators (`generate_boolish_condarc_fixtures.py`
et al.), this section isn't about one parameter's coercion rules -- it's
about a *structural* rule that spans exactly two keys at a time: conda's
`ParameterLoader.raw_parameters_from_single_source` raises
`MultipleKeysError` whenever 2+ of a single parameter's aliased names
(`{canonical_name, *aliases}`) appear together **within the same source
file**. `PAIRS` below is every `(canonical, alias)` pair documented in
§4's catalog (§1.3's own examples -- `always_yes`/`yes`,
`channels`/`channel`, `auto_update_conda`/`self_update` -- plus every
other aliased parameter in the catalog; `root_dir`/`root_prefix` are
listed together since both are aliases of the same internal
`_root_prefix` attribute, so a collision between just the two of them is
equally a `MultipleKeysError`).

## Why the invalid fixtures are NOT lumped like the boolish batteries

`tests/condarc_conformance.rs` automatically *explodes* every
object-shaped fixture under `conformance/condarc/invalid/` into one
single-key case per top-level key (see that file's module docs) -- unless
the fixture's filename ends in `_combined` (before `.json`), in which case
the whole document is checked as one atomic case instead.

A `MultipleKeysError` fixture is invalid *only* in combination: any one of
its two colliding keys, checked alone, is perfectly valid on its own. If
these fixtures were written as ordinary (non-`_combined`) multi-key
objects -- e.g. one giant `{"always_yes": ..., "yes": ..., "channels":
..., "channel": ..., ...}` battery in the boolish style -- the harness's
automatic per-key explosion would silently turn every single resulting
case into a spurious *pass* for `expect_valid=false` (a lone `"yes": ...`
key is valid!), never actually exercising the alias-collision rule at
all. So every invalid fixture here:

  1. Uses the `_combined` filename suffix, opting out of that explosion.
  2. Is deliberately minimal -- exactly the two colliding names for one
     pair, nothing else -- so there's exactly one thing under test per
     fixture and no risk of an unrelated key's own rejection masking
     whether the *alias collision* itself was actually detected.
  3. Is written once per pair (20 fixtures) rather than lumped into a
     handful of shared-battery files, since lumping multiple *different*
     pairs' collisions into one `_combined` document would only prove
     "the combined document as a whole is invalid" -- it wouldn't
     independently confirm that each pair's own collision is what's
     doing the rejecting (a checker could reject the whole document for
     an unrelated reason and every pair would look "covered" without
     ever really being exercised).

## The valid side

The complementary claim -- aliases are genuinely interchangeable when
they *don't* collide -- is a whole-document acceptance property (every
key in the document must be individually fine), which is exactly what
`valid_condarc_is_accepted` already checks without any exploding. So,
matching the boolish batteries' own "one shared document" shape, this
generates exactly two lumped valid fixtures: one spelling every aliased
parameter in `PAIRS` using its *alias* name, and one spelling all of them
using their *canonical/user-facing* name -- proving both spellings work,
individually, for every pair, with no two names of the same pair ever
appearing together.

Every fixture (valid and invalid) is verified empirically against a real
`conda` installation before being written, via the same oracle
`tests/condarc_conformance.rs` uses -- see that module's docs and
`generate_boolish_condarc_fixtures.py`'s docstring for `find_conda_python`
resolution order.

Usage:
    python3 scripts/generate_alias_multiplekeys_condarc_fixtures.py
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
VALID_DIR = REPO_ROOT / "conformance" / "condarc" / "valid"
INVALID_DIR = REPO_ROOT / "conformance" / "condarc" / "invalid"

VALID_ALIAS_FIXTURE = "aliases_accept_alias_spellings_all_params.json"
VALID_CANONICAL_FIXTURE = "aliases_accept_canonical_spellings_all_params.json"
INVALID_PREFIX = "multiple_keys_error_"

# Every `(canonical, alias, value)` triple documented in
# docs/condarc_research.md §4's catalog as having at least one alias.
# `value` is a single representative, structurally-valid value for the
# parameter's declared type -- shared by both spellings, since
# `MultipleKeysError` fires purely because *both names* are present in
# one source file, regardless of what either is set to.
#
# `always_copy`/`copy` and `always_softlink`/`softlink` are each listed
# with a value of their own, but never combined with each other in the
# same document here (only within their own pair's 2-key fixture) --
# `always_copy`+`always_softlink` both truthy is a *separate*,
# independently-modeled cross-field rule (§1.4), not this one, and mixing
# the two rules into the same fixture would conflate them.
PAIRS: list[tuple[str, str, object]] = [
    ("channels", "channel", ["conda-forge"]),
    ("allowlist_channels", "whitelist_channels", ["conda-forge"]),
    ("add_anaconda_token", "add_binstar_token", True),
    ("envs_dirs", "envs_path", ["/tmp/envs"]),
    ("client_ssl_cert", "client_cert", "/tmp/cert.pem"),
    ("client_ssl_cert_key", "client_cert_key", "/tmp/cert-key.pem"),
    ("ssl_verify", "verify_ssl", True),
    ("auto_update_conda", "self_update", True),
    ("disallowed_packages", "disallow", ["numpy"]),
    ("prefix_data_interoperability", "pip_interop_enabled", True),
    ("solver", "experimental_solver", "libmamba"),
    ("always_copy", "copy", True),
    ("always_softlink", "softlink", False),
    ("always_yes", "yes", True),
    ("auto_activate_base", "auto_activate", True),
    ("verbosity", "verbose", 1),
    ("export_platforms", "extra_platforms", ["linux-64"]),
    ("override_virtual_packages", "virtual_packages", {"cuda": "11.0"}),
    ("root_prefix", "root_dir", "/opt/conda"),
    ("environment_specifier", "env_spec", None),
]

CONDA_CHECK_SCRIPT = """
import sys
from conda.base.context import reset_context, context

path = sys.argv[1]
try:
    reset_context(search_path=(path,))
    context.validate_all()
except Exception as e:
    print(f"{type(e).__name__}: {e}", file=sys.stderr)
    sys.exit(1)
sys.exit(0)
"""


def conda_free_env() -> dict[str, str]:
    """Environment with every CONDA*-prefixed variable stripped, so the
    oracle only ever sees the single fixture file passed via search_path."""
    return {k: v for k, v in os.environ.items() if not k.startswith("CONDA")}


def python_has_conda(python: str) -> bool:
    try:
        result = subprocess.run(
            [python, "-c", "import conda"],
            capture_output=True,
            env=conda_free_env(),
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def find_conda_python() -> str:
    """Mirrors tests/condarc_conformance.rs's find_conda_python()."""
    env_python = os.environ.get("ALLEZ_CONFORMANCE_PYTHON")
    if env_python:
        if python_has_conda(env_python):
            return env_python
        sys.exit(
            f"$ALLEZ_CONFORMANCE_PYTHON={env_python!r} does not have `conda` importable"
        )

    if python_has_conda("python3"):
        return "python3"

    conda_path = shutil.which("conda")
    if conda_path:
        conda_dir = os.path.dirname(conda_path)
        for name in ("python3", "python"):
            candidate = os.path.join(conda_dir, name)
            if os.path.isfile(candidate) and python_has_conda(candidate):
                return candidate

    sys.exit(
        "no python interpreter with `conda` importable found (checked "
        "$ALLEZ_CONFORMANCE_PYTHON, `python3` on PATH, and the interpreter "
        "shipped alongside `conda` on PATH) -- this generator requires the "
        "same conda oracle tests/condarc_conformance.rs uses"
    )


def check_candidate(python: str, doc: dict) -> tuple[bool, str]:
    """Returns (is_valid, reason). reason is empty when valid."""
    text = json.dumps(doc)
    fd, path = tempfile.mkstemp(suffix=".yml")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(text)
        result = subprocess.run(
            [python, "-c", CONDA_CHECK_SCRIPT, path],
            capture_output=True,
            text=True,
            env=conda_free_env(),
            timeout=30,
        )
    finally:
        os.unlink(path)

    if result.returncode == 0:
        return True, ""
    return False, result.stderr.strip()


def generate_valid_fixtures(python: str) -> None:
    print("--- valid: alias interchangeability (no collisions) ---\n")

    for stale_name in (VALID_ALIAS_FIXTURE, VALID_CANONICAL_FIXTURE):
        stale_path = VALID_DIR / stale_name
        if stale_path.exists():
            stale_path.unlink()

    alias_doc = {alias: value for _canonical, alias, value in PAIRS}
    canonical_doc = {canonical: value for canonical, _alias, value in PAIRS}

    for filename, doc, label in (
        (VALID_ALIAS_FIXTURE, alias_doc, "alias spellings"),
        (VALID_CANONICAL_FIXTURE, canonical_doc, "canonical spellings"),
    ):
        is_valid, reason = check_candidate(python, doc)
        if is_valid:
            (VALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            print(f"WROTE   {filename}  ({label}, {len(doc)} keys)")
        else:
            print(
                f"SKIPPED {filename}  ({label}): unexpectedly REJECTED by conda: "
                f"{reason}"
            )
            print(
                "        this indicates PAIRS' documented alias/value no longer "
                "matches real conda -- investigate before relying on this fixture."
            )


def generate_invalid_combined_fixtures(python: str) -> tuple[list[str], list[str]]:
    print("\n--- invalid: MultipleKeysError, one minimal _combined fixture per pair ---\n")

    stale = sorted(INVALID_DIR.glob(f"{INVALID_PREFIX}*_combined.json"))
    for path in stale:
        path.unlink()
    if stale:
        print(f"removed {len(stale)} previously-generated {INVALID_PREFIX!r} fixture(s)\n")

    written = []
    unexpectedly_valid = []
    for canonical, alias, value in PAIRS:
        doc = {canonical: value, alias: value}
        filename = f"{INVALID_PREFIX}{canonical}_and_{alias}_combined.json"
        is_valid, reason = check_candidate(python, doc)
        if not is_valid:
            if "MultipleKeysError" not in reason:
                print(
                    f"WARNING {filename}: rejected, but not via MultipleKeysError "
                    f"as expected -- got: {reason[:120]}"
                )
            (INVALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  ({canonical}+{alias}): {reason[:100]}")
        else:
            unexpectedly_valid.append(filename)
            print(
                f"SKIPPED {filename}  ({canonical}+{alias}): unexpectedly ACCEPTED "
                "by conda -- these two names may not actually alias the same "
                "parameter; double-check docs/condarc_research.md §4."
            )

    print(
        f"\n{len(written)} fixture(s) written, {len(unexpectedly_valid)} "
        "candidate(s) skipped (unexpectedly valid).\n"
    )
    return written, unexpectedly_valid


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    generate_valid_fixtures(python)
    _written, unexpectedly_valid = generate_invalid_combined_fixtures(python)

    return 1 if unexpectedly_valid else 0


if __name__ == "__main__":
    raise SystemExit(main())
