#!/usr/bin/env python3
"""Generate the exhaustive `channel_settings_accept_*` /
`channel_settings_reject_*` `.condarc` fixture battery.

## Why `channel_settings` gets its own script

Per `docs/condarc_research.md` §4.1/§6, `channel_settings` is declared in
`conda/base/context.py` as:

    channel_settings = ParameterLoader(
        SequenceParameter(MapParameter(PrimitiveParameter("", element_type=str)))
    )

-- a **list of string-to-string maps**, structurally distinct from every
other container-typed key surveyed so far (plain `SequenceParameter(str)`
list-of-strings keys, or flat `MapParameter(str)` dict keys). It's also
the *only* `.condarc` key whose documented contract (settings.rst: "Each
entry ... needs to define the `channel` key") is explicitly **not**
enforced anywhere in `context.py`/`common/configuration.py` -- see §6.
That combination (novel container shape + a documented-but-unenforced
per-item contract) makes it worth its own dedicated, from-scratch research
pass rather than folding it into the generic list-of-strings/dict-of-str
batteries, per the chat request that produced this script.

## Spelunking: what actually *consumes* `channel_settings` at runtime

`context.py`/`common/configuration.py` place **zero** structural
constraints on a map entry's keys or values beyond "keys and values must
Python-`str`-coerce" (see "Type-coercion findings" below) -- no required
`channel` key, no closed key vocabulary, no length limit. The *only*
place in conda's own source that actually reads back out of
`context.channel_settings` at runtime is
`conda/gateways/connection/session.py`'s `get_session()`:

    for settings in context.channel_settings:
        channel = settings.get("channel", "")
        ...
    auth_handler = channel_settings.get("auth", "").strip() or None
    ...
    auth_handler_cls = context.plugin_manager.get_auth_handler(auth_handler)
    ...
    return CondaSession(auth=auth_handler_cls(channel_name))

So, empirically confirmed by reading this call site end to end:

  - **`channel`**: matched against the request URL two ways -- first an
    *exact* string match against the channel name conda already resolved
    from the URL, then (if no exact match) a `scheme`-must-match,
    `fnmatch()`-glob-pattern match against `netloc + path` (enabling
    settings.rst's `"https://some.base-url-prefix/*"` wildcard example).
    Structurally, still just an arbitrary string at parse time -- the
    glob/URL semantics are entirely a *read-side* interpretation, not
    something `context.py` validates.
  - **`auth`**: looked up via `.get("auth", "").strip() or None`, then
    passed as a plugin-registry *name* to `context.plugin_manager
    .get_auth_handler(auth_handler)`. If no plugin registers that name,
    `get_auth_handler` returns `None` and `get_session()` silently falls
    back to a default, unauthenticated `CondaSession()` -- **an unknown
    `auth` handler name is not a `.condarc`-parse-time error, nor even a
    runtime error**, it just silently no-ops.
  - **Every other key is never read by conda core.** In particular,
    **`user`** -- from settings.rst's own worked example (`user:
    my-user-account`) -- is not referenced anywhere in `conda/`'s own
    Python source. It's a convention for third-party auth-handler
    *plugins* to look up themselves (a `ChannelAuthBase` subclass only
    ever receives `channel_name` as a constructor argument --
    `conda/plugins/types.py`'s `ChannelNameMixin.__init__` -- so a plugin
    wanting a `user`/`password`/anything-else field has to re-fetch
    `context.channel_settings` itself, keyed by channel, exactly the way
    `get_session()` does). This confirms §6's suspicion: the "`channel`
    is required" contract is, at most, a plugin-ecosystem convention that
    individual auth-handler plugins are free to enforce (or not) on their
    own -- conda's own core neither requires nor even inspects it besides
    that one optional lookup-by-value.

None of this is reachable from a `.condarc`-parse-time conformance check
(there's no plugin registered in this suite's oracle invocation, and
`get_session()` isn't called by `validate_all()` at all) -- it's recorded
here purely so a future contributor knows *why* the accept battery below
includes both a `channel`-less map and the full three-key settings.rst
shape as equally "valid", instead of assuming the missing-`channel` case
was an oversight.

## Type-coercion / structural findings (all empirically confirmed against
a real `conda` installation, mirroring every other generator here)

1. **A raw JSON `null` for the whole key is treated as unset**, exactly
   like every other `SequenceParameter`-typed key (`docs/condarc_research
   .md`'s list-of-strings survey, finding 2): `{"channel_settings": null}`
   resolves to `()`, indistinguishable from omitting the key.
2. **An empty JSON object (`{}`) for the whole key is silently treated as
   an empty list**, same "emptiness" pattern as other sequence-typed keys.
   **A *non-empty* JSON object for the whole key, however, hits a crash
   bug that is *new* relative to every previously-documented crash
   (`docs/condarc_research.md` §8 items 7/8/9), and specific to
   `channel_settings`'s doubly-nested shape**: `SequenceParameter.load()`
   iterates a raw dict's *keys* (plain Python `str`, never wrapped) as if
   they were list elements (the same root-level list/dict confusion bug
   documented in §1.1, recurring one level down), then calls
   `MapParameter.load(name, that_plain_string)`, which immediately calls
   `match.value(...)` on the plain string -- raising an unhandled
   `AttributeError: 'str' object has no attribute 'value'`. This is a
   distinct message/call-site from both §8 item 7's `'YamlRawParameter'
   object has no attribute 'typify'` and item 8's `issubclass()` crash --
   worth keeping distinct if a future contributor asserts on conda's
   exact error text.
3. **A list element that isn't a map at all (string/int/float/bool) is
   cleanly rejected** with `InvalidTypeError` ("has type str/int/.../
   Valid types: - frozendict") -- `MapParameter.load()` checks
   `isinstance(value, Mapping)` *before* ever recursing into per-key
   `.load()` calls, so this never reaches the crash-prone code path.
   This includes a **raw list-shaped element** too (`[[]]`/`[[1, 2]]`) --
   unlike the analogous "nested list *inside* a list-of-strings element"
   case (which crashes for non-empty content per §8 item 5), a list
   element here is caught by that same early `isinstance(..., Mapping)`
   check regardless of emptiness, since a `tuple` is never a `Mapping` --
   **so this is a case where `channel_settings` is *more* crash-resistant
   than the plain list-of-strings keys, not less**, precisely because the
   type check happens one call-frame earlier.
4. **A raw `null` *list element* (as opposed to a `null` whole-key) is
   silently treated as an empty map, not rejected and not preserved as
   `None`** -- `MapParameter.load()` explicitly special-cases `value is
   None` and returns an empty `MapLoadedParameter` (the same code path
   that makes `proxy_servers: ~`-shaped per-key nulls valid elsewhere).
   `[null]` therefore loads successfully as `(frozendict({}),)`.
5. **A map *value* that's itself a nested, non-empty list or dict crashes**
   with the already-documented `AttributeError: 'YamlRawParameter' object
   has no attribute 'typify'` (§8 item 5) -- this is one container level
   *deeper* than finding 3 above: the outer list-element-is-a-map check
   passes fine, and the crash instead happens while typifying that map's
   *value* against its declared `str` element type. An *empty* nested
   list/dict value does not crash -- it fails cleanly instead
   (`InvalidTypeError`, "has type tuple/frozendict, Valid types: - str"),
   the same emptiness-gated split documented elsewhere. This holds even
   when the crashing key is just one of several keys in an otherwise
   well-formed map (`{"channel": "x", "extra": [1, 2]}` still crashes).
6. **Every map *value* that Python's `str()` constructor accepts silently
   coerces**, exactly like every other `str`-typed leaf in this codebase:
   ints/floats/bools/`null` all stringify (`5 -> "5"`, `True -> "True"`,
   `None -> "None"`), and pre-existing `str` values pass through
   whitespace-and-all unchanged (the same `isinstance(value, str) and
   issubclass(type_hint, str)` short-circuit documented in §2.1).
7. **Map keys and values both tolerate arbitrary string content**
   (empty string, whitespace-only, Unicode, control characters, NUL
   bytes, quotes/backslashes, arbitrarily long strings) with zero
   validation -- `channel_settings` declares no `validation=` callable
   at any level (§3 lists none), so there's no scheme requirement (unlike
   `channel_alias`), no closed vocabulary (unlike `list_fields`), nothing.
8. **Exact-duplicate map entries within a single list are silently
   deduplicated at merge time**, down to `len() == 1` even for two
   *structurally* identical maps in the raw list (confirmed: `[{"channel":
   "x"}, {"channel": "x"}]` loads with `len(context.channel_settings) ==
   1`, while `[{"channel": "x"}, {"channel": "y"}]` keeps both). This
   falls out of `SequenceLoadedParameter.merge()`'s generic `unique(...)`
   de-dup pass (`conda/common/configuration.py`) -- `LoadedParameter.
   __eq__`/`__hash__` compare on `.value`, so two structurally-equal
   `MapLoadedParameter`s collide even within one file's single sequence
   match, not just across merged sources. This is a real conda semantic
   quirk (probably surprising to a user expecting two independent
   per-channel overrides to both apply), but it is **not** an "invalid"
   outcome for this suite's pass/fail purposes -- duplicates remain
   `channel_settings_accept_*`, just noted here so a future contributor
   isn't surprised by the collapsed length if they ever assert on the
   *value*, not just accept/reject.

Every candidate (both accept and reject) is verified empirically against
a real `conda` installation before a fixture is written, for the same
self-correcting reason as every other generator in this directory.

Usage:
    python3 scripts/generate_channel_settings_condarc_fixtures.py

Requires a Python interpreter with `conda` importable. Resolution order
matches `tests/condarc_conformance.rs`'s `find_conda_python`:
$ALLEZ_CONFORMANCE_PYTHON, then `python3` on PATH, then the interpreter
shipped alongside whatever `conda` executable is on PATH.
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

KEY = "channel_settings"

# ---------------------------------------------------------------------
# Accept battery -- see module docstring's numbered findings for the
# reasoning behind each group.
# ---------------------------------------------------------------------
ACCEPT: list[tuple[str, object]] = [
    # --- size 0 ---
    ("array_size_0_empty", []),
    # An empty *object* for the whole key is silently treated as an empty
    # list -- finding 2. Paired with `object_nonempty` in REJECT below.
    ("object_size_0_empty_treated_as_array", {}),
    # --- size 1: map shape / documented-vs-undocumented keys ---
    ("array_size_1_empty_map", [{}]),
    ("array_size_1_channel_only", [{"channel": "https://example.com/chan"}]),
    # settings.rst says every entry "needs" a `channel` key, but nothing
    # in context.py enforces it -- see the "Spelunking" section above.
    ("array_size_1_missing_channel_key", [{"auth": "some-handler"}]),
    # The full three-key shape straight from settings.rst's own example.
    (
        "array_size_1_settingsrst_documented_shape",
        [
            {
                "channel": "https://some.custom/channel",
                "auth": "test-auth-handler",
                "user": "my-user-account",
            }
        ],
    ),
    # Arbitrary undocumented keys are structurally accepted too -- there
    # is no closed key vocabulary at parse time.
    (
        "array_size_1_arbitrary_undocumented_key",
        [{"channel": "x", "totally_made_up_key": "z"}],
    ),
    ("array_size_1_many_keys_in_one_map", [{f"k{i}": f"v{i}" for i in range(20)}]),
    # --- size 1: value coercion (bare str() call on the map's values) ---
    ("array_size_1_value_coerced_int", [{"channel": 5}]),
    ("array_size_1_value_coerced_negative_int", [{"channel": -5}]),
    ("array_size_1_value_coerced_float", [{"channel": 1.5}]),
    ("array_size_1_value_coerced_negative_float", [{"channel": -5.5}]),
    ("array_size_1_value_coerced_bool_true", [{"channel": True}]),
    ("array_size_1_value_coerced_bool_false", [{"channel": False}]),
    ("array_size_1_value_coerced_null", [{"channel": None}]),
    # --- size 1: string content edge cases (keys and values) ---
    ("array_size_1_key_empty_string", [{"": "value"}]),
    ("array_size_1_value_empty_string", [{"channel": ""}]),
    ("array_size_1_value_whitespace_only", [{"channel": "   "}]),
    ("array_size_1_value_whitespace_tabs_and_newlines", [{"channel": "\t\n"}]),
    ("array_size_1_key_unicode_cjk", [{"频道": "x"}]),
    ("array_size_1_value_unicode_cjk", [{"channel": "日本語"}]),
    ("array_size_1_value_unicode_emoji", [{"channel": "😀🎉"}]),
    ("array_size_1_key_special_chars", [{"my.key:with/chars": "x"}]),
    ("array_size_1_value_control_chars_tab_newline", [{"channel": "a\tb\nc"}]),
    ("array_size_1_value_null_byte", [{"channel": "a\0b"}]),
    ("array_size_1_value_quotes_and_backslash", [{"channel": "a\"b'c\\d"}]),
    ("array_size_1_value_very_long_string", [{"channel": "x" * 10000}]),
    # The settings.rst wildcard-pattern example -- structurally just a
    # plain string at parse time; the fnmatch()-glob semantics are a
    # read-side interpretation in session.py's get_session(), not
    # anything context.py validates.
    (
        "array_size_1_value_glob_pattern_wildcard",
        [{"channel": "https://some.base-url-prefix/*", "auth": "another-auth-handler"}],
    ),
    # --- a null *element* (not a null whole-key) is a distinct, valid
    # edge case -- finding 4: silently becomes an empty map. ---
    ("array_element_null_treated_as_empty_map", [None]),
    # --- size 2 ---
    ("array_size_2_distinct_channels", [{"channel": "x"}, {"channel": "y"}]),
    # Structurally-identical entries are still unconditionally valid --
    # they just collapse to length 1 at merge time (finding 8). Still
    # "accepted", which is all this suite's pass/fail oracle cares about.
    ("array_size_2_duplicate_identical_maps", [{"channel": "x"}, {"channel": "x"}]),
    ("array_size_2_two_null_elements", [None, None]),
    ("array_size_2_null_then_valid_map", [None, {"channel": "x"}]),
    # --- larger ---
    ("array_size_50_many_entries", [{"channel": f"c{i}"} for i in range(50)]),
    # --- whole-key null (distinct from an array *containing* null) ---
    ("null_literal_treated_as_unset", None),
]

# ---------------------------------------------------------------------
# Reject battery -- see module docstring's numbered findings.
# ---------------------------------------------------------------------
REJECT: list[tuple[str, object]] = [
    # Bare scalars for the whole key -- sequence-typed keys require a
    # real list (or, per finding 2, an empty dict) at the raw level.
    ("bare_string", "just-a-string"),
    ("bare_empty_string", ""),
    ("bare_int", 5),
    ("bare_float", 5.5),
    ("bare_bool_true", True),
    ("bare_bool_false", False),
    # Paired with `object_size_0_empty_treated_as_array` in ACCEPT: only
    # the *empty* dict is silently valid -- a non-empty one hits the
    # *new*, channel_settings-specific crash documented in finding 2
    # (`AttributeError: 'str' object has no attribute 'value'`, distinct
    # from every previously-documented crash bug).
    ("object_nonempty", {"channel": "x"}),
    # List elements that aren't maps at all -- cleanly rejected before
    # any crash-prone recursion (finding 3).
    ("array_element_string", ["just-a-string"]),
    ("array_element_empty_string", [""]),
    ("array_element_int", [5]),
    ("array_element_float", [5.5]),
    ("array_element_bool_true", [True]),
    ("array_element_bool_false", [False]),
    # A list-shaped element (not a map, not a scalar) -- also cleanly
    # rejected regardless of emptiness (finding 3), unlike the analogous
    # nested-list-inside-a-list-of-strings-element case elsewhere, which
    # crashes when non-empty.
    ("array_nested_empty_array_element", [[]]),
    ("array_nested_nonempty_array_element", [[1, 2]]),
    # A map *value* that's a nested container -- crashes when non-empty,
    # rejects cleanly when empty (finding 5). One level deeper than the
    # previous group: the map itself is well-formed, only one of its
    # values isn't a valid `str`-coercible leaf.
    ("map_value_nested_empty_array", [{"channel": []}]),
    ("map_value_nested_nonempty_array", [{"channel": [1, 2]}]),
    ("map_value_nested_empty_object", [{"channel": {}}]),
    ("map_value_nested_nonempty_object", [{"channel": {"a": 1}}]),
    # The crash still fires even when the offending value shares a map
    # with an otherwise perfectly valid `channel` key.
    (
        "map_value_nested_nonempty_array_alongside_valid_key",
        [{"channel": "x", "extra": [1, 2]}],
    ),
    # A well-formed map alongside a non-map element -- the whole document
    # is still invalid; a checker that only looks at the first element
    # would wrongly call this valid.
    ("array_mixed_valid_map_and_invalid_scalar", [{"channel": "x"}, 5]),
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


def clear_stale(directory: Path, prefix: str) -> None:
    stale = sorted(directory.glob(f"{prefix}*.json"))
    for path in stale:
        path.unlink()
    if stale:
        print(f"removed {len(stale)} previously-generated {prefix!r} fixture(s)")


def generate_accept_battery(python: str) -> tuple[list[str], list[str]]:
    prefix = f"{KEY}_accept_"
    clear_stale(VALID_DIR, prefix)

    written: list[str] = []
    unexpected: list[str] = []
    for slug, value in ACCEPT:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if is_valid:
            (VALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            unexpected.append(filename)
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly REJECTED "
                f"by conda ({reason[:150]})"
            )
    return written, unexpected


def generate_reject_battery(python: str) -> tuple[list[str], list[str]]:
    prefix = f"{KEY}_reject_"
    clear_stale(INVALID_DIR, prefix)

    written: list[str] = []
    unexpected: list[str] = []
    for slug, value in REJECT:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if not is_valid:
            (INVALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r}): {reason[:150]}")
        else:
            unexpected.append(filename)
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly ACCEPTED "
                "by conda -- this candidate belongs in the accept battery instead"
            )
    return written, unexpected


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    total_written = 0
    total_unexpected = 0

    print(f"--- {KEY}_accept_* ---\n")
    written, unexpected = generate_accept_battery(python)
    total_written += len(written)
    total_unexpected += len(unexpected)
    print(
        f"\n{len(written)} fixture(s) written, {len(unexpected)} candidate(s) "
        "skipped (unexpected outcome).\n"
    )

    print(f"--- {KEY}_reject_* ---\n")
    written, unexpected = generate_reject_battery(python)
    total_written += len(written)
    total_unexpected += len(unexpected)
    print(
        f"\n{len(written)} fixture(s) written, {len(unexpected)} candidate(s) "
        "skipped (unexpected outcome).\n"
    )

    print(
        f"=== total: {total_written} fixture(s) written, "
        f"{total_unexpected} skipped ==="
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
