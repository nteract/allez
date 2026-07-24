#!/usr/bin/env python3
"""Generate the exhaustive `boolish_values_reject_*` / `nullable_values_reject_*`
fixture batteries.

Sibling of `generate_boolish_condarc_fixtures.py` (which generates the
*valid* `boolish_values_accept_*` / `nullable_values_accept_*` batteries)
-- see that script's docstring for the full breakdown of `KEYS` (Category
A "plain bool" + Category B "nullable bool" + Category C `ssl_verify`,
per `docs/condarc_research.md`'s bool-like-key catalog), and for why
`local_repodata_ttl` and (from the shared battery only) `always_softlink`
are deliberately excluded.

Same "apply one candidate value to all of `KEYS` at once" shape as the
valid battery, but for values that make the combined `.condarc` document
*rejected*.

Because `Context.validate_all()` collects errors across *every*
parameter and raises if *any* of them is invalid, a candidate value only
needs to fail for **at least one** key in `KEYS` (when applied to all of
them) for the whole document to be rejected -- unlike the valid battery,
which requires *unanimous* acceptance. See `docs/condarc_research.md` §8
items 7-9 for the full writeup, including two genuine conda **crash
bugs** (unhandled `TypeError`s, not clean validation errors) this
generator's own investigation turned up:

  - item 7: a *non-empty* raw list/dict value fed to any of these
    scalar-typed keys crashes with `AttributeError` (an *empty* list/dict
    does not -- it's a real, meaningful sub-case, hence both are covered
    below). This applies uniformly across Category A, B, and C -- every
    `PrimitiveParameter`-backed key behaves this way regardless of its
    `element_type`'s shape.
  - item 8: any string that's neither a `boolify()` token nor parseable by
    Python's `complex()` crashes `LoadedParameter.typify()`'s error
    handler with `TypeError: issubclass() arg 1 must be a class` -- but
    **only** via a *tuple* `element_type`, i.e. only via one of the four
    nullable `(bool, NoneType)` keys (Category B) or `ssl_verify`'s
    `(str, bool)` (Category C, though `ssl_verify`'s `return_string=True`
    passthrough means it doesn't actually reach this crash for strings --
    see item 9). Empirically confirmed while expanding this script to
    include Category A: the exact same candidate (e.g. `"banana"`)
    applied to a *plain*, non-nullable `bool` key (Category A) does
    **not** crash -- `issubclass(bool, Enum)` is a perfectly legal call
    since `bool` is a single class, not a tuple -- it raises a clean
    `CustomValidationError` ("cannot be boolified") instead. Either way
    the candidate is invalid for the corresponding key and the shared
    battery still correctly lands the whole document in `invalid/`; only
    the underlying mechanism differs (crash vs. clean rejection).
  - item 9: `ssl_verify`'s custom filesystem-existence validation rejects
    any string that neither boolifies to a real `bool` nor happens to be
    an existing path/`"truststore"` -- e.g. the string `"null"` (which
    *does* boolify to `None` for the four nullable keys, but stays a raw,
    unboolified string for non-nullable `ssl_verify` and then fails that
    path check). For Category A (plain, non-nullable `bool`) keys,
    `"null"` fails even earlier -- `boolify(..., nullable=False)` itself
    raises `TypeCoercionError` for it (see `NULLABLE_ONLY_REJECT_
    CANDIDATES` below) -- so it's invalid for Category A too, just via yet
    another distinct mechanism.

**`NULLABLE_ONLY_REJECT_CANDIDATES`** -- a second, separate battery -- is
the mirror image of `generate_boolish_condarc_fixtures.py`'s
`NULLABLE_ONLY_CANDIDATES`: the same `NULL_STRINGS`-but-not-`BOOLISH_
FALSE` tokens (`"null"`, `"~"`, `"\0"`), but applied to **only the
Category A (`PLAIN_BOOL_KEYS`) keys -- the four nullable keys (Category B)
are deliberately excluded** from this battery's document, since those
same tokens are exactly what makes them succeed (become `None`) rather
than fail. Precision matters here the same way it did for pulling
`local_repodata_ttl` out of the shared battery (§8 item 11): a document
that mixed Category A and Category B keys for these specific tokens would
still show up "invalid" (because of the Category A keys), but would
muddy whether that invalidity is actually caused by the
nullability-sensitive tokens or by something else entirely.

Every candidate is still verified empirically against a real `conda`
installation before a fixture is written -- if a candidate unexpectedly
turns out to be *accepted* (e.g. the several candidates that were moved
to the valid battery once `local_repodata_ttl` was pulled out, per §8
item 11), it is reported as SKIPPED rather than silently written to
`invalid/`.

Usage:
    python3 scripts/generate_boolish_condarc_reject_fixtures.py

Requires a Python interpreter with `conda` importable; see the module
docstring of `generate_boolish_condarc_fixtures.py` for resolution order.
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
INVALID_DIR = REPO_ROOT / "conformance" / "condarc" / "invalid"
FIXTURE_PREFIX = "boolish_values_reject_"
NULLABLE_FIXTURE_PREFIX = "nullable_values_reject_"

# Category A: the plain, non-nullable `bool` element_type `Context`
# parameters -- see docs/condarc_research.md §4 ("Category A") and
# generate_boolish_condarc_fixtures.py's module docstring for the full
# rationale (including why `always_softlink` is held back from the shared
# battery only).
PLAIN_BOOL_KEYS = [
    "override_channels_enabled",
    "add_anaconda_token",
    "allow_non_channel_urls",
    "no_lock",
    "repodata_use_zst",
    "repodata_use_shards",
    "offline",
    "auto_update_conda",
    "force_reinstall",
    "prefix_data_interoperability",
    "allow_softlinks",
    "always_copy",
    "always_softlink",
    "rollback_enabled",
    "extra_safety_checks",
    "shortcuts",
    "non_admin_enabled",
    "separate_format_cache",
    "auto_activate_base",
    "changeps1",
    "json",
    "notify_outdated_conda",
    "quiet",
    "unsatisfiable_hints",
    "envvars_force_uppercase",
    "allow_cycles",
    "allow_conda_downgrades",
    "add_pip_as_python_dependency",
    "debug",
    "trace",
    "dev",
    "enable_private_envs",
    "force_32bit",
    "solver_ignore_timestamps",
    "register_envs",
    "protect_frozen_envs",
    "no_plugins",
]

# Unlike the accept battery, the invalid/reject battery has no
# cross-field-rule hazard: a candidate that fails to *coerce* at all never
# reaches `Context.post_build_validation()`'s always_copy/always_softlink
# mutual-exclusivity check, so `always_softlink` does not need to be
# excluded here.
PLAIN_BOOL_KEYS_IN_BATTERY = PLAIN_BOOL_KEYS

# Category B: the four `(bool, NoneType)` nullable keys.
NULLABLE_BOOL_KEYS = [
    "use_only_tar_bz2",
    "always_yes",
    "report_errors",
    "show_channel_urls",
]

# Category C: kept for continuity with the original battery.
SSL_VERIFY_KEY = "ssl_verify"

# The full shared "apply one value to every key simultaneously" battery:
# Category A + Category B + Category C.
KEYS = PLAIN_BOOL_KEYS_IN_BATTERY + NULLABLE_BOOL_KEYS + [SSL_VERIFY_KEY]

# (slug, value) -- see the module docstring and docs/condarc_research.md
# §8 items 7-9 for why each of these is expected to be rejected, and by
# which specific mechanism.
CANDIDATES: list[tuple[str, object]] = [
    # Collections -- empty vs non-empty is a meaningful sub-case here
    # (different rejection mechanism: clean MultiValidationError for
    # empty, crash for non-empty -- §8 item 7). Not specific to any one
    # key -- every PrimitiveParameter-backed key behaves this way.
    ("array_empty", []),
    ("array_nonempty", [1, 2, 3]),
    ("object_empty", {}),
    ("object_nonempty", {"a": 1}),
    # An arbitrary English word: not a boolify() token, not complex()
    # -parseable -- triggers the issubclass() crash (§8 item 8) via any of
    # the four nullable (bool, NoneType) keys; rejected cleanly (no crash)
    # via any of the Category A plain-bool keys instead.
    ("string_arbitrary_word", "banana"),
    # A hex-literal-shaped string: also not parseable by complex(),
    # triggers the same issubclass() crash (§8 item 8) as "banana" above --
    # kept as its own fixture since it's a distinctly-shaped string
    # (previously this crashed via a *different* bug, local_repodata_ttl's
    # hex()-misuse, before that key was pulled out -- see §8 items 9/11).
    ("string_hex_literal", "0x1A"),
]

# NOTE: an earlier revision of this shared battery also included
# `("string_null_token", "null")` here, applied to the *full* `KEYS` list.
# That was a genuine bug, not just redundant: "null" is a `NULL_STRINGS`
# token but NOT a `BOOLISH_FALSE` token, so it boolifies to a real Python
# `None` -- and is therefore *individually valid* -- for the four
# nullable `(bool, NoneType)` keys in `NULLABLE_BOOL_KEYS` (confirmed by
# `nullable_values_accept_string_null_token.json`). Applying it to the
# shared, non-`_combined` `KEYS` list made `invalid_condarc_is_rejected`'s
# per-key explosion (tests/condarc_conformance.rs) assert that
# `{"always_yes": "null"}` alone should be rejected, when real conda
# actually accepts it -- a latent failure that stayed hidden only because
# of an unrelated stale-test-binary caching issue (rstest's `#[files(...)]`
# glob runs at compile time; adding/removing fixtures alone didn't used to
# trigger a rebuild -- now fixed via `make`'s `touch tests/condarc_
# conformance.rs`). "null"'s two *genuinely* key-scoped rejection
# mechanisms are already covered elsewhere and don't need re-adding here:
# `NULLABLE_ONLY_REJECT_CANDIDATES` below (Category A only) and
# `generate_ssl_verify_passthrough_reject_fixtures.py`'s own
# `ssl_verify_passthrough_reject_string_null_token.json` (`ssl_verify`
# only).

# See module docstring: the mirror image of generate_boolish_condarc_
# fixtures.py's NULLABLE_ONLY_CANDIDATES, applied only to the Category A
# (non-nullable) keys -- the four nullable keys are deliberately excluded.
NULLABLE_ONLY_REJECT_CANDIDATES: list[tuple[str, object]] = [
    ("string_null_token", "null"),
    ("string_tilde_token", "~"),
    ("string_null_byte_token", "\0"),
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


def generate_battery(
    python: str,
    keys: list[str],
    candidates: list[tuple[str, object]],
    prefix: str,
) -> tuple[list[str], list[str]]:
    """Shared driver for a single "apply one candidate value to every key
    in `keys` simultaneously" reject battery. Returns (written,
    unexpectedly_valid)."""
    stale = sorted(INVALID_DIR.glob(f"{prefix}*.json"))
    for path in stale:
        path.unlink()
    if stale:
        print(f"removed {len(stale)} previously-generated {prefix!r} fixture(s)\n")

    written = []
    unexpectedly_valid = []
    for slug, value in candidates:
        doc = {key: value for key in keys}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if not is_valid:
            out_path = INVALID_DIR / filename
            out_path.write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r}): {reason[:100]}")
        else:
            unexpectedly_valid.append(filename)
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly ACCEPTED "
                "by conda -- this candidate belongs in the valid battery instead"
            )

    print(
        f"\n{len(written)} fixture(s) written, {len(unexpectedly_valid)} "
        "candidate(s) skipped (unexpectedly valid).\n"
    )
    return written, unexpectedly_valid


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    print(f"--- {FIXTURE_PREFIX}* (Category A + B + C, {len(KEYS)} keys) ---\n")
    generate_battery(python, KEYS, CANDIDATES, FIXTURE_PREFIX)

    print(
        f"--- {NULLABLE_FIXTURE_PREFIX}* (Category A only, "
        f"{len(PLAIN_BOOL_KEYS)} keys, nullable keys deliberately excluded) ---\n"
    )
    generate_battery(
        python,
        PLAIN_BOOL_KEYS,
        NULLABLE_ONLY_REJECT_CANDIDATES,
        NULLABLE_FIXTURE_PREFIX,
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
