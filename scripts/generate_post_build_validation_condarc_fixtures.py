#!/usr/bin/env python3
"""Generate fixtures for `docs/condarc_research.md` §1.4 ("Cross-field /
semantic validation (`Context.post_build_validation`)").

`Context.post_build_validation()` enforces exactly two rules *after* every
individual parameter has already loaded/coerced successfully -- these are
not per-key type/enum checks, they're relationships *between* two already-
valid keys:

  1. `client_ssl_cert_key` is set (truthy) but `client_ssl_cert` is not ->
     `ValidationError("client_ssl_cert' is required when 'client_ssl_cert_key'
     is defined")`. Note the direction: only `client_ssl_cert_key` implies
     `client_ssl_cert` -- the reverse (cert without a key) is fine, and
     `client_ssl_cert_key` being *falsy* (unset, `null`, or `""`) never
     triggers this at all, regardless of `client_ssl_cert`.
  2. `always_copy` and `always_softlink` are **both** truthy ->
     `ValidationError("'always_copy' and 'always_softlink' are mutually
     exclusive. Only one can be set to 'True'.")`. Either alone is fine;
     only the *combination* is rejected.

## Why the invalid fixtures use `_combined` (or are naturally single-key)

Same rationale as `generate_alias_multiplekeys_condarc_fixtures.py` (see
that script's docstring and `tests/condarc_conformance.rs`'s module docs):
`invalid_condarc_is_rejected` automatically explodes every object-shaped
fixture into one single-key case per top-level key, *unless* the filename
ends in `_combined` -- because a checker that rejects a whole multi-key
document only proves *some* key is invalid, not that every key is.

  - Rule 2's fixtures are invalid **only in combination**: `always_copy`
    alone, or `always_softlink` alone, is perfectly valid. Both `_combined`
    fixtures below (`always_copy_and_softlink_combined.json`, spelled with
    canonical names, and its alias-spelled sibling) opt out of the
    automatic explosion accordingly.
  - Rule 1's fixtures are invalid from a **single key's own value**
    (`client_ssl_cert_key` truthy) -- `client_ssl_cert`'s *absence* is what
    makes it invalid, not the presence of some second key that could be
    exploded away. These fixtures are therefore written as ordinary
    single-key objects (no `_combined` suffix needed): exploding a
    one-key object is a no-op, so there's no risk of the harness hiding
    anything by splitting it.

`always_copy_and_softlink_combined.json` is the pre-existing fixture named
directly in `tests/condarc_conformance.rs`'s own module-doc example of the
`_combined` naming convention -- this script continues to own and
regenerate it in place (same filename) rather than duplicating its exact
content under a new name.

## The valid side

Mirrors each rule's *true* boundary, not just "the opposite of invalid":
cert-without-key, an explicitly-falsy `client_ssl_cert_key` (`""`/`null`)
with no cert at all, either `always_copy`/`always_softlink` alone, both
rules satisfied at once in one document, and alias-spelled variants of
each -- proving the rules key off the *coerced parameter*, not the
specific spelling used to set it (consistent with §1.3's aliasing rules).

Every fixture (valid and invalid) is verified empirically against a real
`conda` installation before being written, via the same oracle
`tests/condarc_conformance.rs` uses.

Usage:
    python3 scripts/generate_post_build_validation_condarc_fixtures.py
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

# (filename, doc, expect_valid, expected_error_substring or None)
#
# expected_error_substring is only checked for invalid cases, as a
# sanity check that rejection is coming from the *cross-field* rule this
# script is about, not some unrelated per-key coercion failure.
CASES: list[tuple[str, dict, bool, str | None]] = [
    # --- Rule 1: client_ssl_cert_key implies client_ssl_cert ---
    (
        "post_build_validation_reject_client_ssl_cert_key_without_cert.json",
        {"client_ssl_cert_key": "/tmp/cert-key.pem"},
        False,
        "client_ssl_cert' is required",
    ),
    (
        "post_build_validation_reject_client_ssl_cert_key_alias_without_cert.json",
        {"client_cert_key": "/tmp/cert-key.pem"},
        False,
        "client_ssl_cert' is required",
    ),
    (
        "post_build_validation_accept_client_ssl_cert_without_key.json",
        {"client_ssl_cert": "/tmp/cert.pem"},
        True,
        None,
    ),
    (
        "post_build_validation_accept_client_ssl_cert_key_empty_string.json",
        {"client_ssl_cert_key": ""},
        True,
        None,
    ),
    (
        "post_build_validation_accept_client_ssl_cert_key_null.json",
        {"client_ssl_cert_key": None},
        True,
        None,
    ),
    (
        "post_build_validation_accept_client_cert_and_key_via_aliases.json",
        {"client_cert": "/tmp/cert.pem", "client_cert_key": "/tmp/cert-key.pem"},
        True,
        None,
    ),
    # --- Rule 2: always_copy / always_softlink mutual exclusivity ---
    # Filename kept stable: this is the pre-existing fixture named
    # directly in tests/condarc_conformance.rs's module docs as the
    # canonical example of the `_combined` naming convention.
    (
        "always_copy_and_softlink_combined.json",
        {"always_copy": True, "always_softlink": True},
        False,
        "mutually exclusive",
    ),
    (
        "post_build_validation_reject_copy_and_softlink_via_aliases_combined.json",
        {"copy": True, "softlink": True},
        False,
        "mutually exclusive",
    ),
    (
        "post_build_validation_accept_always_copy_only.json",
        {"always_copy": True},
        True,
        None,
    ),
    (
        "post_build_validation_accept_always_softlink_only.json",
        {"always_softlink": True},
        True,
        None,
    ),
    # --- Both rules satisfied simultaneously, in one document ---
    (
        "post_build_validation_accept_both_rules_satisfied_simultaneously.json",
        {
            "client_ssl_cert": "/tmp/cert.pem",
            "client_ssl_cert_key": "/tmp/cert-key.pem",
            "always_copy": True,
            "always_softlink": False,
        },
        True,
        None,
    ),
]

# Every filename this script owns -- used to detect + remove stale
# fixtures from a prior run whose CASES entry has since been renamed or
# dropped, so re-running this script stays idempotent the same way the
# other generate_*.py scripts are (glob-by-prefix-and-clear). Cross-field
# fixtures don't share one filename prefix (the always_copy/softlink one
# is deliberately unprefixed, per the docstring), so we track the exact
# filename set instead of a glob.
OWNED_FILENAMES = {filename for filename, _doc, _valid, _err in CASES}

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


def target_dir_for(filename: str, expect_valid: bool) -> Path:
    return VALID_DIR if expect_valid else INVALID_DIR


def clear_stale_fixtures() -> None:
    """Removes any previously-generated fixture (in either directory)
    whose filename this script owns but which no longer appears in
    CASES -- e.g. after a case is renamed."""
    for directory in (VALID_DIR, INVALID_DIR):
        for path in directory.glob("post_build_validation_*.json"):
            if path.name not in OWNED_FILENAMES:
                path.unlink()
                print(f"removed stale fixture: {path.relative_to(REPO_ROOT)}")


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    clear_stale_fixtures()

    mismatches = []
    for filename, doc, expect_valid, expected_err in CASES:
        is_valid, reason = check_candidate(python, doc)
        target_dir = target_dir_for(filename, expect_valid)

        if is_valid != expect_valid:
            mismatches.append(filename)
            print(
                f"MISMATCH {filename}: expected "
                f"{'valid' if expect_valid else 'invalid'}, conda says "
                f"{'valid' if is_valid else 'invalid'} ({reason}) -- NOT WRITTEN"
            )
            continue

        if not expect_valid and expected_err and expected_err not in reason:
            print(
                f"WARNING {filename}: rejected as expected, but the error "
                f"didn't mention {expected_err!r} -- got: {reason[:120]}"
            )

        target_dir.mkdir(parents=True, exist_ok=True)
        (target_dir / filename).write_text(json.dumps(doc, indent=2) + "\n")
        verdict = "valid" if expect_valid else "invalid"
        detail = reason[:100] if reason else ""
        print(f"WROTE   {filename}  ({verdict}){': ' + detail if detail else ''}")

    print(f"\n{len(CASES) - len(mismatches)} fixture(s) written, {len(mismatches)} mismatch(es).\n")
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())
