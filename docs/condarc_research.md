# `.condarc` Research Notes (GEN-36)

**Status**: living research document, written before `docs/condarc_openapi.json`'s
properties are fully built out. This is a plain research/notes file, not a
Spec Kit `spec.md` — it exists so the full-fidelity findings backing
`docs/condarc_openapi.json` and `conformance/condarc/**` are recorded
somewhere durable, instead of being rebuilt from scratch (or lost) partway
through incrementally building the schema.

**Primary sources** (conda `main` branch, read directly from
`raw.githubusercontent.com/conda/conda/main/...` on 2026-07-22):

- `docs/source/user-guide/configuration/settings.rst` — the user-facing docs page linked from GEN-36.
- `conda/base/context.py` — the `Context(Configuration)` class: the authoritative list of every recognized `.condarc` parameter, its declared Python type, default, aliases, and any per-parameter validation callable.
- `conda/common/configuration.py` — the generic `Configuration`/`Parameter`/`ParameterLoader`/`RawParameter` machinery that actually reads YAML, merges multiple sources, coerces types, and raises validation errors.
- `conda/auxlib/type_coercion.py` — the `typify()`/`boolify()`/`numberify()` functions that perform the actual runtime type coercion referenced by `common/configuration.py`.
- `conda/base/constants.py` — enum definitions (`ChannelPriority`, `PathConflict`, `SafetyChecks`, `SatSolverChoice`, `DepsModifier`, `UpdateModifier`) and default constants (`DEFAULT_CHANNELS`, `DEFAULT_CUSTOM_CHANNELS`, `DEFAULT_CHANNEL_ALIAS`, `DEFAULT_SOLVER`, `DEFAULT_AGGRESSIVE_UPDATE_PACKAGES`, `KNOWN_SUBDIRS`, `CONDA_LIST_FIELDS`, `NO_PLUGINS`, `RESERVED_ENV_NAMES`, `SEARCH_PATH`, etc).

Some findings below were also **empirically verified** against a locally
installed `conda` (same `main`-branch codebase) by a research sub-agent
actually calling `typify()`/`boolify()`/`Context()`/etc. rather than only
reading source; those are called out explicitly as "empirically confirmed."

---

## 1. Document-level structure

### 1.1 Root type

A `.condarc` file is YAML. Its root **must be a mapping** for anything useful
to happen. Concretely, in `conda/common/configuration.py`:

- `YamlRawParameter.make_raw_parameters_from_file()` calls `yaml.loads(fh)` to
  get `yaml_obj`, then `YamlRawParameter.make_raw_parameters(source, yaml_obj)`,
  which does:
  ```python
  if from_map:
      return {key: cls(source, key, from_map[key], ...) for key in from_map}
  return EMPTY_MAP
  ```
- **Empty file, or a bare YAML `~`/`null` root** → `yaml_obj` is `None`, which
  is falsy → `make_raw_parameters` returns `EMPTY_MAP` (an empty frozendict).
  **No error, no warning** — this is silently treated as "no settings in this
  file," exactly as if the file didn't exist.
- **A list root** (e.g. the whole file is `- a\n- b`) → `yaml_obj` is a
  (truthy) list. `make_raw_parameters` then does `for key in from_map:
  ...from_map[key]`, iterating the **list's elements** as if they were dict
  keys, then indexing the list with those (string) elements → raises a raw,
  uncaught **`TypeError: list indices must be integers or slices, not str`**.
  This propagates all the way out of `Context()`/`Configuration.__init__()` —
  it is *not* wrapped in `ConfigurationLoadError` or any conda-specific
  exception type.
- **A scalar root** (e.g. the whole file is just `just a string`) → same
  shape of bug, different message: iterating the string's characters as
  "keys" and indexing the string with them → raw, uncaught **`TypeError:
  string indices must be integers`**.
- Only **YAML syntax errors** (`ScannerError`/`ReaderError` from `ruamel.yaml`)
  are caught explicitly inside `make_raw_parameters_from_file` and converted
  into a clean `ConfigurationLoadError` (`"Unable to load configuration
  file.\n  path: %(path)s\n  reason: invalid yaml at line %(line)s, column
  %(column)s"`), which `_load_search_path` further catches per-file and
  degrades to a logged warning + skipped file. A **structurally-valid YAML
  document with the wrong root *type*** does not get this graceful
  treatment — it's an unhandled crash.

### 1.2 Unrecognized / unknown top-level keys

Empirically confirmed: simply loading a `.condarc` (via `Context(search_path=(path,))`) containing an unrecognized key (e.g. a typo like `always_yess: true`, or any key that isn't the canonical name or a declared alias of any `Context` parameter) raises **no error and produces no warning**.

Why: every `Context` parameter (`ParameterLoader`) only ever looks up its *own* canonical name + declared aliases in each source's raw key set (`Parameter.get_all_matches` → `ParameterLoader.raw_parameters_from_single_source`, which computes `keys = names & frozenset(raw_parameters.keys())`). Nothing in `Configuration`/`Context` ever iterates a *file's* keys and cross-checks them against the known parameter set. `validate_all()`/`collect_all()`/`check_source()` all iterate `self.parameter_names` (the known set), never the raw file's keys. There is no "unknown key" detection or warning anywhere in `common/configuration.py` or `base/context.py`.

(Separately, `conda config --describe`/`conda config --validate`/`conda config --show-sources` CLI plumbing in `conda/cli/main_config.py` may do additional user-facing linting — that file was out of scope for this research pass and was not read. But plain "does the file load" behavior, which is GEN-36's actual concern, tolerates unknown keys silently.)

**Modeling decision**: `additionalProperties: true` at the schema root (already the scaffold's initial choice) is confirmed correct.

### 1.3 Aliases and `MultipleKeysError`

Many parameters have one or more aliases (e.g. `always_yes` aliases to `yes`; `_channels` aliases to `channels`/`channel`; `auto_update_conda` aliases to `self_update`). A single parameter's full "names" set is `{canonical_name, *aliases}`.

- `MultipleKeysError` fires **only when 2+ of a single parameter's aliased names appear together within the same single source/file**. `ParameterLoader.raw_parameters_from_single_source` is called once per source (once per file in the search path); if a *single file* defines e.g. both `always_yes: true` and `yes: false`, that raises `MultipleKeysError` ("Multiple aliased keys in file ...: - yes - always_yes. Must declare only one. Prefer 'always_yes'").
- If the *same* alias collision instead spans **two different files** in the search path (one file sets `always_yes`, another sets `yes`), there is **no error** — both are collected as independent per-source matches and merged normally (last-one-wins per conda's standard multi-source precedence rules), exactly like any other legitimate override across files.
- Note the internal wrinkle: for many parameters, the class attribute name registered as the parameter's own "canonical name" in `Context` is actually a *leading-underscore* internal name (e.g. `_channels`, `_root_prefix`, `_console`, `_debug`, `_verbosity`, `_use_only_tar_bz2`, `_report_errors`, `_error_upload_url`, `_croot`, `_conda_build`, `_override_virtual_packages`, `_channel_alias`, `_create_default_packages`, `_default_activation_env`, `_aggressive_update_packages`, `_signing_metadata_url_base`, `_default_threads`, `_repodata_threads`, `_fetch_threads`, `_verify_threads`, `_execute_threads`, `_migrated_channel_aliases`, `_custom_channels`, `_custom_multichannels`, `_default_channels`, `_subdir`, `_subdirs`, `_export_platforms`, `_trace`). The user-facing key that actually appears in real `.condarc` files is always one of the *aliases* (e.g. `channels`, `root_dir`/`root_prefix`, `console`, `debug`, `verbosity`/`verbose`, `use_only_tar_bz2`, `report_errors`, `error_upload_url`, `croot`, `conda-build`/`conda_build`, `override_virtual_packages`/`virtual_packages`, `channel_alias`, `create_default_packages`, `default_activation_env`, `aggressive_update_packages`, `signing_metadata_url_base`, `default_threads`, `repodata_threads`, `fetch_threads`, `verify_threads`, `execute_threads`, `migrated_channel_aliases`, `custom_channels`, `custom_multichannels`, `default_channels`, `subdir`, `subdirs`, `export_platforms`/`extra_platforms`, `trace`). The literal underscored name (e.g. `_channels`) is *technically* also in that parameter's `.names` set and would therefore also work as a raw `.condarc` key, but no real-world `.condarc` file does this and it's treated here as an implementation artifact, not part of the documented surface.
- `docs/condarc_openapi.json`'s properties are keyed by the *user-facing* canonical/alias name (matching settings.rst and real-world usage), not the underscored internal attribute name. Aliases are recorded per-property via the `x-conda-aliases` extension field for traceability, but (given `additionalProperties: true`) an alias key used in a fixture is currently only checked as "some untyped extra property," not type-checked against the same schema as its canonical name. This is a deliberate, documented scope limitation — see §7.

### 1.4 Cross-field / semantic validation (`Context.post_build_validation`)

Beyond per-key type/enum checks, `Context.post_build_validation()` enforces exactly two cross-field rules after all individual parameters have loaded:

1. `client_ssl_cert_key` is set (truthy) **but** `client_ssl_cert` is not → `ValidationError("client_ssl_cert", ..., "'client_ssl_cert' is required when 'client_ssl_cert_key' is defined")`.
2. `always_copy` and `always_softlink` are **both** truthy → `ValidationError("always_copy", ..., "'always_copy' and 'always_softlink' are mutually exclusive. Only one can be set to 'True'.")`.

These are the *only* two cross-parameter rules in `Context` itself (as opposed to per-parameter `validation=` callables, §3). Both are naturally expressible as JSON Schema `not`/`allOf` combinators and are modeled that way.

---

## 2. Type coercion engine (`conda/auxlib/type_coercion.py`)

This is the layer that turns a raw YAML-parsed Python value into the
type declared by a parameter's `element_type`. It runs (via
`LoadedParameter.typify()` → `typify_data_structure()` → `typify()`) *after*
raw values are merged across sources, and *before* `collect_errors()`'s
`isinstance(typed_value, self._type)` check — so successful coercion means
no type error is ever raised, even if the *raw* value's Python type doesn't
match the declared type at all.

### 2.1 `typify(value, type_hint=None)` dispatch table

| `type_hint` shape | Behavior |
|---|---|
| `None` | `typify_str_no_hint(value)`: regex-guesses bool/`None`/int/float/complex from a string, else leaves it as a string unchanged. |
| a single concrete type `T` (not a tuple), `T is bool` | `boolify(value)` (non-nullable). |
| a single concrete type `T` (not a tuple), `T` otherwise | `T(value)` (plain Python constructor call), `ValueError` → `TypeCoercionError`. **Important**: a bare Python `TypeError` (e.g. `int(None)`, `int([])`, `int({})`) is *not* caught by this `except ValueError` clause and propagates as an unhandled crash, exactly like the root-type bug in §1.1. |
| a tuple of types that is an `Enum` subclass wrapped as a 1-tuple, or the type_hint itself an `Enum` subclass | `type_hint(value)` (value-based lookup) → on `ValueError`, falls back to `type_hint[value]` (name-based lookup) → on `KeyError`, `TypeCoercionError`. See §2.3. |
| tuple of types, set-equal to `{int, float, complex}` | `numberify(value)`. |
| tuple of types, set-equal to `{str}` | `str(value)`. |
| tuple of types, set-equal to `{bool, NoneType}` | `boolify(value, nullable=True)`. |
| tuple of types, set-equal to `{str, bool}` | `boolify(value, return_string=True)` (unboolifiable strings pass through unchanged instead of erroring). |
| tuple of types, set-equal to `{str, NoneType}` | `str(value)`, except the literal string `"none"` (case-insensitively) becomes `None`. |
| tuple of types, set-equal to `{bool, int}` | `typify_str_no_hint(str(value))` (regex-guess only). |
| anything else | `NotImplementedError` (not expected to occur for any `Context` parameter as declared today). |

Note the set-difference checks are **exact type-object equality**, not
subclass-aware — even though `bool` is a Python subclass of `int`, `{bool,
NoneType} - {int, float, complex}` does **not** remove `bool` (since `bool
!= int` as distinct type objects), so the `{bool, NoneType}` branch and the
numeric branches never collide.

Also note: for the plain-string no-hint path, `typify()` first does
`value.strip()` if `value` is a `str` — meaning leading/trailing whitespace
is silently trimmed for untyped/string-coerced values, but **not** for
values explicitly typed as `str` via `typify_data_structure` (which special-cases
`isinstance(value, str) and issubclass(type_hint, str)` to skip `typify()`
entirely and return the value unchanged, whitespace and all).

### 2.2 `boolify()` — exact truth tables

```python
BOOLISH_TRUE  = ("true", "yes", "on", "y")
BOOLISH_FALSE = ("false", "off", "n", "no", "non", "none", "")
NULL_STRINGS  = ("none", "~", "null", "\0")
BOOL_COERCEABLE_TYPES = (int, bool, float, complex, list, set, dict, tuple)
```

`boolify(value, nullable=False, return_string=False)`:

1. `isinstance(value, BOOL_COERCEABLE_TYPES)` → return `bool(value)` (plain
   Python truthiness). This means **any** JSON number, list, or object —
   not just `0`/`1` — coerces via truthiness: `42 → True`, `-1 → True`, `0 →
   False`, `[] → False`, `[1,2,3] → True` (non-empty!), `{} → False`, `{"a":
   1} → True` (non-empty!).
2. Else (value is a string not already caught above, or possibly `None` —
   `NoneType` is *not* in `BOOL_COERCEABLE_TYPES`), stringify + lowercase +
   strip, then strip a trailing/internal `.`-related numeric check: if
   `val.isnumeric()` → `bool(float(val))` (so `"0"→False`, `"0.0"` — not
   numeric per `str.isnumeric()`'s stricter rules — falls through instead;
   `"2"→True`).
3. `val in BOOLISH_TRUE` (`"true","yes","on","y"`, case-folded) → `True`.
4. If `nullable` and `val in NULL_STRINGS` (`"none","~","null","\0"`) →
   `None`. (`str(None).lower()` is `"none"`, which is in `NULL_STRINGS` —
   this is exactly how a JSON `null` fed into a `(bool, NoneType)`-typed
   field round-trips back to `None` rather than erroring or becoming
   `False`.)
5. `val in BOOLISH_FALSE` (`"false","off","n","no","non","none",""`) →
   `False`.
6. Else, try `bool(complex(val))` (accepts numeric-and-complex-looking
   strings like `"1+2j"`); if that *also* raises, then: if the original
   `value` was a `str` and `return_string=True`, return the original string
   **unchanged** (this is what makes e.g. `ssl_verify`-shaped `{str, bool}`
   fields tolerate arbitrary non-boolish path strings without erroring);
   otherwise raise `TypeCoercionError(value, f"The value {value!r} cannot be
   boolified.")`.

Concrete consequence for a `(bool, NoneType)`-typed field (e.g. `always_yes`,
`show_channel_urls`, `use_only_tar_bz2`, `report_errors`): the **only** JSON
values that actually *fail* to load are strings that are simultaneously (a)
not numeric, (b) not one of the boolish-true/false/null tokens above, and
(c) not parseable as a Python `complex` literal — e.g. `"banana"`,
`"enabled"`, `"yes please"`. A JSON number, JSON array, or JSON object value
for such a field is **not** rejected by real conda — it's silently
truthiness-coerced. (See §7 for why this schema deliberately does not model
that leniency.)

### 2.3 Enum coercion — exact rule

For an `Enum`-subclass `type_hint` (e.g. `ChannelPriority`, `SafetyChecks`,
`PathConflict`, `DepsModifier`, `UpdateModifier`, `SatSolverChoice`):

1. Try `type_hint(value)` — **value-based** lookup (matches the member's
   `.value` string). Case-sensitive, exact string match only.
2. On `ValueError`, try `type_hint[value]` — **name-based** lookup (matches
   the member's Python identifier/`.name`, e.g. `STRICT`, `NOT_SET`). Also
   case-sensitive, exact match only.
3. If both fail, raise `TypeCoercionError`.

So an enum-typed field accepts **either** its documented lowercase value
string **or** its (often SHOUTY_CASE) Python member name, but nothing
case-folded or fuzzy in between. E.g. for `ChannelPriority` (`STRICT =
"strict"`, `FLEXIBLE = "flexible"`, `DISABLED = "disabled"`): `"strict"` ✅
(value match), `"STRICT"` ✅ (name match, since the member's name literally
is `STRICT`), `"Strict"` ❌ (matches neither). For `SafetyChecks` (`enabled =
"enabled"`, `warn = "warn"`, `disabled = "disabled"` — name and value
identical, both lowercase): `"enabled"` ✅, `"ENABLED"` ❌ (fails both;
the member's actual *name* is lowercase `enabled`, not `ENABLED`).

**`ChannelPriority` has one more special case** via a custom `EnumMeta`
subclass (`ChannelPriorityMeta`), preserving conda's historical
boolean-valued `channel_priority: true`/`channel_priority: false` config
(from before it became a 3-way enum):

```python
class ChannelPriorityMeta(EnumMeta):
    def __call__(cls, value, *args, **kwargs):
        try:
            return super().__call__(value, *args, **kwargs)
        except ValueError:
            if isinstance(value, str):
                value = typify(value)  # no-hint: regex-guess bool/None/number
            if value is True:
                value = "flexible"
            elif value is False:
                value = cls.DISABLED
            return super().__call__(value, *args, **kwargs)
```

So: literal JSON `true` → `ChannelPriority.FLEXIBLE`; literal JSON `false` →
`ChannelPriority.DISABLED`; and (via the inner `typify()` no-hint call) even
the *strings* `"true"`/`"yes"`/`"on"` → regex-matched to Python `True` →
`FLEXIBLE`, and `"false"`/`"no"`/`"off"` → Python `False` → `DISABLED`. No
other enum in `Context` has an analogous historical bool-compat shim — this
is unique to `ChannelPriority`.

### 2.4 Sequence/Map fields do **not** auto-wrap scalars

`type_coercion.py` performs no scalar→sequence or scalar→map coercion.
Whether a sequence-typed `.condarc` key accepts a bare scalar is actually
decided one layer up, in `SequenceParameter.load()`
(`common/configuration.py`): it checks `isiterable(value)` on the *raw*
merged value and raises `InvalidTypeError` if the raw value isn't already
iterable — e.g. `channels: "just-a-string"` (a bare YAML string, itself
technically iterable character-by-character in Python, but **`isiterable()`
is conda's own predicate**, not `hasattr(value, "__iter__")` — empirically,
a bare string raw value for a sequence-typed parameter is rejected: `channels:
"just-a-string"` → `InvalidTypeError: Parameter _channels = 'just-a-string'
... has type str. Valid types: - tuple`). So: **a sequence-typed setting
requires a real YAML/JSON list at the raw level; a bare scalar is rejected,
not auto-wrapped into a single-element list.**

`typify_data_structure()` (the *element*-level typify step, applied once the
raw value is confirmed to already be the right outer container type) simply
maps `typify(element, element_type)` over each list item / dict value —
it does not alter the container shape itself.

---

## 3. Per-parameter custom `validation=` callables

A handful of `Context` parameters attach an extra `validation` callable
(beyond the base type-coercion/`isinstance` check), invoked from
`LoadedParameter.collect_errors()` on the already-coerced `typed_value`:

- **`channel_alias`** (`channel_alias_validation`): if the (non-empty)
  string value has no URL scheme (`has_scheme(value)` is `False`) → error
  `"channel_alias value '{value}' must have scheme/protocol."`. An empty
  string is allowed (bypasses the check entirely).
- **`default_python`** (`default_python_validation`): empty string or falsy
  → valid (means "no python pinning"). Otherwise: the string must have
  `value[1] == '.'` (i.e., a single-digit major version immediately followed
  by a literal `.`) **and** `float(value)` must land in `[2.0, 4.0)`. Note
  this is a fairly loose check — e.g. `"3.10"` passes only because
  `float("3.10") == 3.1`, which is still `< 4.0`; it does not actually parse
  "3" and "10" as separate major/minor components. Failure message:
  `"default_python value '{value}' not of the form '[23].[0-9][0-9]?' or
  ''"`.
- **`list_fields`** (`list_fields_validation`): every element of the
  sequence must be one of the fixed known keys in `CONDA_LIST_FIELDS`
  (§5.9); anything else → `"Invalid value(s): [...]. Valid values are:
  [...]"`.
- **`ssl_verify`** (`ssl_verify_validation`): only runs meaningful checks
  when the coerced value is a `str` (a `bool` value skips straight through,
  always valid). For a string value: if it's exactly `"truststore"`, valid
  only on Python ≥ 3.10 (else: `"ssl_verify: truststore is only supported on
  Python 3.10 or later"`); otherwise, the string **must be a path that
  exists on the local filesystem** (`os.path.exists(value)`) or else:
  `"ssl_verify value '{value}' must be a boolean, a path to a certificate
  bundle file, a path to a directory containing certificates of trusted CAs,
  or 'truststore' to use the operating system certificate store."`. This
  filesystem-existence check is **inherently environment-dependent and not
  something a portable, hermetic conformance suite can encode** — see §7 for
  how this schema handles it.

No other parameter declares a custom `validation=` callable in
`context.py` as of this research pass.

---

## 4. Full parameter catalog

Below, every `ParameterLoader` attribute in `Context` (from `conda/base/context.py`) is listed with: the **user-facing key** (the alias actually meant to be written in a real `.condarc`, if the class attribute itself is internal/underscored — see §1.3), any **other aliases**, the **Python `element_type`** exactly as declared, the **documented/coerced default**, and **notes**. Grouped following `Context.category_map`'s own grouping (a purely doc/`conda config --describe` grouping, not a functional restriction — nothing stops any of these from being set in a real `.condarc`, including the "CLI-only" and "Hidden and Undocumented" ones).

Legend: *nullable* = `element_type` is a 2-tuple `(T, NoneType)`, i.e. a JSON `null` is a first-class, non-coerced accepted value, not merely an artifact of `boolify`'s NULL_STRINGS handling.

### 4.1 Channel Configuration

| user-facing key | aliases | type | default | notes |
|---|---|---|---|---|
| `channels` | `channel` | `SequenceParameter(str)` | `()` | settings.rst: default is `["defaults"]` in practice via the `defaults` multichannel name, but the raw parameter's own default is an empty tuple. |
| `channel_alias` | — | `str`, custom validation | `"https://conda.anaconda.org"` (`DEFAULT_CHANNEL_ALIAS`) | must have a URL scheme unless empty; see §3. |
| `channel_settings` | — | `SequenceParameter(MapParameter(str))` | `()` | list of maps; settings.rst documents each entry as needing a `channel` key, but this is **not** enforced by any type-level or `validation=` check found in `context.py` — it's a documentation-level convention, not a hard parse-time requirement. Modeled here as `"required": ["channel"]` per the docs, flagged as *not independently source-verified as a hard requirement*. |
| `default_channels` | — | `SequenceParameter(str)` | `DEFAULT_CHANNELS` (platform-dependent, §5.5) | |
| `override_channels_enabled` | — | `bool` | `True` | |
| `allowlist_channels` | `whitelist_channels` | `SequenceParameter(str)` | `()` | |
| `denylist_channels` | — | `SequenceParameter(str)` | `()` | |
| `custom_channels` | — | `MapParameter(str)` | `DEFAULT_CUSTOM_CHANNELS` = `{"pkgs/pro": "https://repo.anaconda.com"}` | map of channel-name → base-URL string. |
| `custom_multichannels` | — | `MapParameter(SequenceParameter(str))` | `{}` | map of multichannel-name → list of channel names/URLs. |
| `migrated_channel_aliases` | — | `SequenceParameter(str)` | `()` | |
| `migrated_custom_channels` | — | `MapParameter(str)` | `{}` | (no alias to a shorter name; class attribute *is* the public name here, unusually not underscored.) |
| `add_anaconda_token` | `add_binstar_token` | `bool` | `True` | |
| `allow_non_channel_urls` | — | `bool` | `False` | |
| `repodata_fns` | — | `SequenceParameter(str)` | `("current_repodata.json", "repodata.json")` | |
| `use_only_tar_bz2` | — | `(bool, NoneType)` *nullable* | `None` | documented default `False`; internal default is `None` (tri-state, like `always_yes`). |
| `repodata_threads` | — | `int` | `0` (means "unset"/`None` at the property level) | |
| `fetch_threads` | — | `int` | `0` (means "unset"; property computes `5` when both this and `default_threads` are `0`) | |
| `experimental` | — | `SequenceParameter(str)` | `()` | free-form list of experimental feature-flag strings; not a closed enum in `context.py`. |
| `no_lock` | — | `bool` | `False` | |
| `repodata_use_zst` | — | `bool` | `True` | |
| `repodata_use_shards` | — | `bool` | `True` | |

### 4.2 Basic Conda Configuration

| user-facing key | aliases | type | default | notes |
|---|---|---|---|---|
| `envs_dirs` | `envs_path` | `SequenceParameter(str)`, `expandvars=True`, `string_delimiter=os.pathsep` | `()` | The `os.pathsep`-based `string_delimiter` only matters for the `EnvRawParameter` (environment-variable) source, which splits a colon/semicolon-delimited *string* into a sequence; for YAML/JSON sources the raw value is already expected to be a real list. |
| `pkgs_dirs` | — | `SequenceParameter(str)`, `expandvars=True` | `()` | |
| `default_threads` | — | `int` | `0` (means "unset"/`None`) | |
| `preview` | — | `SequenceParameter(str)` | `()` | free-form list of preview feature-flag strings. |

### 4.3 Network Configuration

| user-facing key | aliases | type | default | notes |
|---|---|---|---|---|
| `client_ssl_cert` | `client_cert` | `(str, NoneType)` *nullable* | `None` | required if `client_ssl_cert_key` is set — cross-field rule, §1.4. |
| `client_ssl_cert_key` | `client_cert_key` | `(str, NoneType)` *nullable* | `None` | |
| `local_repodata_ttl` | — | `(bool, int)` (**not** nullable — a 2-tuple of concrete non-`NoneType` types) | `1` | `True`/`1` = respect `Cache-Control`; `False`/`0` = always refetch; any other positive int = seconds to cache. |
| `offline` | — | `bool` | `False` | |
| `proxy_servers` | — | `MapParameter(PrimitiveParameter((str, NoneType)))`, `expandvars=True` | `{}` | map of scheme/`scheme://host` → proxy URL string, or `null`. |
| `remote_connect_timeout_secs` | — | `float` | `9.15` | |
| `remote_max_retries` | — | `int` | `3` | |
| `remote_backoff_factor` | — | `int` (declared via bare `PrimitiveParameter(1)`, so `element_type = type(1) = int`, **not** `float`, despite the name suggesting it could be fractional) | `1` | |
| `remote_read_timeout_secs` | — | `float` | `60.0` | |
| `ssl_verify` | `verify_ssl` | `(str, bool)`, custom validation, `expandvars=True` | `True` | see §3 for the filesystem-existence caveat. |

### 4.4 Solver Configuration

| user-facing key | aliases | type | default | notes |
|---|---|---|---|---|
| `aggressive_update_packages` | — | `SequenceParameter(str)` | `DEFAULT_AGGRESSIVE_UPDATE_PACKAGES` = `("ca-certificates", "certifi", "openssl")` | list of `MatchSpec`-parseable strings; only structural (string) validation happens at config-load time, `MatchSpec` parsing happens lazily via a `@property`. |
| `auto_update_conda` | `self_update` | `bool` | `True` | |
| `channel_priority` | — | `ChannelPriority` enum, custom metaclass coercion | `ChannelPriority.FLEXIBLE` | see §2.3 for the historical bool-compat shim. |
| `create_default_packages` | — | `SequenceParameter(str)` | `()` | |
| `disallowed_packages` | `disallow` | `SequenceParameter(str)`, `string_delimiter="&"` | `()` | the `&` delimiter is (like `envs_dirs`'s `os.pathsep`) only relevant to the env-var source, not YAML/JSON. |
| `force_reinstall` | — | `bool` | `False` | |
| `pinned_packages` | — | `SequenceParameter(str)`, `string_delimiter="&"` | `()` | **BETA** per settings.rst/description_map. |
| `prefix_data_interoperability` | `pip_interop_enabled` | `bool` | `False` | |
| `track_features` | — | `SequenceParameter(str)` | `()` | |
| `solver` | `experimental_solver` | `str` (**plain string, not a closed `Enum`**) | `DEFAULT_SOLVER` = `"libmamba"` | despite settings.rst implying a fixed choice, this is *not* a Python `Enum` in `context.py` — any string is structurally accepted; solver *plugins* can register arbitrary names (`"classic"` and `"libmamba"` are merely the two built-in values). |

### 4.5 Package Linking and Install-time Configuration

| user-facing key | aliases | type | default | notes |
|---|---|---|---|---|
| `allow_softlinks` | — | `bool` | `False` | |
| `always_copy` | `copy` | `bool` | `False` | mutually exclusive with `always_softlink` — cross-field rule, §1.4. |
| `always_softlink` | `softlink` | `bool` | `False` | |
| `path_conflict` | — | `PathConflict` enum | `PathConflict.clobber` | values: `clobber`, `warn`, `prevent`. |
| `rollback_enabled` | — | `bool` | `True` | |
| `safety_checks` | — | `SafetyChecks` enum | `SafetyChecks.warn` | values: `enabled`, `warn`, `disabled` (member name == member value for all three, all lowercase). |
| `extra_safety_checks` | — | `bool` | `False` | |
| `signing_metadata_url_base` | — | `(str, NoneType)` *nullable* | `None` | |
| `shortcuts` | — | `bool` | `True` | |
| `shortcuts_only` | — | `SequenceParameter(str)`, `expandvars=True` | `()` | list of package names to restrict shortcut creation to. |
| `non_admin_enabled` | — | `bool` | `True` | |
| `separate_format_cache` | — | `bool` | `False` | |
| `verify_threads` | — | `int` | `0` (property defaults effective value to `1` when unset) | |
| `execute_threads` | — | `int` | `0` (property defaults effective value to `1` when unset) | |

### 4.7 Output, Prompt, and Flow Control Configuration

| user-facing key | aliases | type | default | notes |
|---|---|---|---|---|
| `always_yes` | `yes` | `(bool, NoneType)` *nullable* | `None` (documented default `False`) | see §2.2 for the coercion boundary. |
| `auto_activate_base` | `auto_activate` (class attribute *is* `auto_activate`, so `auto_activate_base` is technically the alias, not the canonical name — but it's the name settings.rst uses historically) | `bool` | `True` | |
| `default_activation_env` | — | `str` (falls back to `ROOT_ENV_NAME` = `"base"` if empty) | `"base"` | |
| `auto_stack` | — | `int` | `0` | `0`/`False` disables; `1`/`True` enables for one level; can be any int in principle. |
| `changeps1` | — | `bool` | `True` | |
| `env_prompt` | — | `str` | `"({default_env}) "` | template string; `{prefix}`/`{name}`/`{default_env}` placeholders, no structural validation of the template beyond being a string. |
| `json` | — | `bool` | `False` | |
| `console` | — | `str` (**plain string, not a closed enum**) | `DEFAULT_CONSOLE_REPORTER_BACKEND` = `"classic"` | reporter-backend name; plugins can register arbitrary values, so (like `solver`) this is intentionally unconstrained at the type level. |
| `notify_outdated_conda` | — | `bool` | `True` | |
| `quiet` | — | `bool` | `False` | |
| `report_errors` | — | `(bool, NoneType)` *nullable* | `None` | **deprecated** (`@deprecated("26.9", "27.3")` on the read-side property) but still a settable/loadable parameter as of this research. |
| `show_channel_urls` | — | `(bool, NoneType)` *nullable* | `None` (documented default `False`) | |
| `list_fields` | — | `SequenceParameter(str)`, custom validation | `("name", "version", "build", "channel_name")` | elements restricted to the closed `CONDA_LIST_FIELDS` key set, §5.9. |
| `verbosity` | `verbose` | `int` | `0` | note: `verbose` is documented/used elsewhere as a *count* flag (`-v`, `-vv`, ...); as a `.condarc` key it's a plain int. |
| `unsatisfiable_hints` | — | `bool` | `True` | |
| `unsatisfiable_hints_check_depth` | — | `int` | `2` (description_map text says "Defaults to 3" — a doc/code mismatch; the actual `ParameterLoader` default literal is `2`) | |
| `number_channel_notices` | — | `int` | `5` | `0` fully suppresses channel notices. |
| `envvars_force_uppercase` | — | `bool` | `True` | |
| `export_platforms` | `extra_platforms` | `SequenceParameter(str)` | `()` | |
| `override_virtual_packages` | `virtual_packages` | `MapParameter((str, NoneType))` | `{}` | map of virtual-package name → version-or-build override string, or `null`. Dunder-prefixed keys (`__cuda`) have their `__` stripped on read, but that's a read-side transform, not a parse-time structural requirement — any string key is structurally accepted. |

### 4.9 Hidden and Undocumented (per `category_map`; still real, loadable parameters)

| user-facing key | aliases | type | default | notes |
|---|---|---|---|---|
| `allow_cycles` | — | `bool` | `True` | allow cyclical dependencies, or raise. |
| `allow_conda_downgrades` | — | `bool` | `False` | |
| `add_pip_as_python_dependency` | — | `bool` | `True` | |
| `debug` | — | `bool` | `False` | class attribute is `_debug`; `debug` is its sole alias and the only realistic `.condarc` spelling. |
| `trace` | — | `bool` | `False` | class attribute `_trace`; same shape as `debug`. |
| `dev` | — | `bool` | `False` | |
| `default_python` | — | `(str, NoneType)` *nullable*, custom validation | current interpreter's `"{major}.{minor}"` | see §3. |
| `enable_private_envs` | — | `bool` | `False` | |
| `error_upload_url` | — | `str` | `"https://conda.io/conda-post/unexpected-error"` | **deprecated** read-side property; class attribute `_error_upload_url`. |
| `force_32bit` | — | `bool` | `False` | |
| `root_dir` / `root_prefix` | (both are aliases of the same underscored `_root_prefix` attribute) | `str` | `""` (falls back to `sys.prefix` if empty) | |
| `sat_solver` | — | `SatSolverChoice` enum | `SatSolverChoice.PYCOSAT` | values: `pycosat`, `pycryptosat`, `pysat`. |
| `solver_ignore_timestamps` | — | `bool` | `False` | |
| `subdir` | — | `str` | `""` (falls back to the native platform subdir if empty) | |
| `subdirs` | — | `SequenceParameter(str)` | `()` (falls back to `(subdir, "noarch")` if empty) | |
| `target_prefix_override` | — | `str` | `""` | |
| `register_envs` | — | `bool` | `True` | |
| `protect_frozen_envs` | — | `bool` | `True` | |

### 4.10 Plugin Configuration

| user-facing key | aliases | type | default |
|---|---|---|---|
| `no_plugins` | — | `bool` | `NO_PLUGINS` = `False` |

### 4.11 Experimental

| user-facing key | aliases | type | default | notes |
|---|---|---|---|---|
| `environment_specifier` | `env_spec` | `(str, NoneType)` *nullable* | `None` | **EXPERIMENTAL** per its own docstring in `description_map`; expect breaking changes upstream. |

---

## 5. Enum & constant exact values

### 5.1 `ChannelPriority` (`conda/base/constants.py`, custom `ChannelPriorityMeta`)
`STRICT = "strict"`, `FLEXIBLE = "flexible"`, `DISABLED = "disabled"`. Plus the bool-compat shim in §2.3 (`True`→`FLEXIBLE`, `False`→`DISABLED`, and boolish *strings* route through the same shim).

### 5.2 `PathConflict`
`clobber = "clobber"`, `warn = "warn"`, `prevent = "prevent"`.

### 5.3 `SafetyChecks`
`disabled = "disabled"`, `warn = "warn"`, `enabled = "enabled"`.

### 5.4 `SatSolverChoice`
`PYCOSAT = "pycosat"`, `PYCRYPTOSAT = "pycryptosat"`, `PYSAT = "pysat"`.

### 5.5 `DepsModifier`
`NOT_SET = "not_set"` (default), `NO_DEPS = "no_deps"`, `ONLY_DEPS = "only_deps"`.

### 5.6 `UpdateModifier`
`SPECS_SATISFIED_SKIP_SOLVE = "specs_satisfied_skip_solve"`, `FREEZE_INSTALLED = "freeze_installed"`, `UPDATE_DEPS = "update_deps"`, `UPDATE_SPECS = "update_specs"` (default), `UPDATE_ALL = "update_all"`.

### 5.7 Default channels
```python
DEFAULT_CHANNEL_ALIAS = "https://conda.anaconda.org"
DEFAULT_CHANNELS_UNIX = ("https://repo.anaconda.com/pkgs/main", "https://repo.anaconda.com/pkgs/r")
DEFAULT_CHANNELS_WIN  = ("https://repo.anaconda.com/pkgs/main", "https://repo.anaconda.com/pkgs/r", "https://repo.anaconda.com/pkgs/msys2")
DEFAULT_CHANNELS = DEFAULT_CHANNELS_WIN if on_win else DEFAULT_CHANNELS_UNIX
DEFAULT_CUSTOM_CHANNELS = {"pkgs/pro": "https://repo.anaconda.com"}
DEFAULTS_CHANNEL_NAME = "defaults"
```

### 5.8 Solver/reporter string defaults (not enums)
```python
DEFAULT_CONSOLE_REPORTER_BACKEND = "classic"
DEFAULT_JSON_REPORTER_BACKEND    = "json"
DEFAULT_SOLVER = "libmamba"
```

### 5.9 `CONDA_LIST_FIELDS` (closed key set for `list_fields`)
```
arch, build, build_number, channel, channel_name, constrains, depends,
dist_str, features, fn, license, license_family, md5, name, noarch,
package_type, requested_spec, requested_specs, sha256, size, subdir,
timestamp, track_features, url, version
```
`DEFAULT_CONDA_LIST_FIELDS = ("name", "version", "build", "channel_name")`.

### 5.10 Misc
```python
DEFAULT_AGGRESSIVE_UPDATE_PACKAGES = ("ca-certificates", "certifi", "openssl")
KNOWN_SUBDIRS = ("noarch", *PLATFORMS)  # PLATFORMS includes linux-64/osx-64/osx-arm64/win-64/win-arm64/... (17 platform subdirs total)
NO_PLUGINS = False
ROOT_ENV_NAME = "base"
RESERVED_ENV_NAMES = ("base", "root")
REPODATA_FN = "repodata.json"
```

---

## 6. `channel_settings` item shape (documented but not source-enforced)

settings.rst's example:
```yaml
channel_settings:
   - channel: https://some.custom/channel
     auth: test-auth-handler
     user: my-user-account
   - channel: https://some.base-url-prefix/*
     auth: another-auth-handler
```
and states: *"Each entry in `channel_settings` needs to define the `channel`
attribute..."* However, `context.py` declares `channel_settings` simply as
`SequenceParameter(MapParameter(PrimitiveParameter("", element_type=str)))`
— a list of string-to-string maps, with **no `validation=` callable** and no
other code path in `context.py`/`common/configuration.py` found that checks
for a `channel` key's presence. The `channel` requirement therefore appears
to be enforced elsewhere (plausibly by the auth-handler plugin machinery
that actually *consumes* `channel_settings` at request time, not at
`.condarc`-parse time) or is simply a documentation convention that isn't
hard-enforced at all at load time. **This was not independently verified
beyond `context.py`/`common/configuration.py`** (the plugin auth-handler
code was out of scope for this pass). The OpenAPI schema models this
per-item `"required": ["channel"]` anyway, following the documented
contract, but flags this uncertainty explicitly via `x-conda-source`.

---

## 8. Addendum: concrete implementation fixes found while wiring up the conformance suite

These are bugs the conformance harness itself caught once
`docs/condarc_openapi.json` and `conformance/condarc/**` were built out and
run against `tests/condarc_conformance.rs` — recorded here since they're
exactly the kind of full-fidelity detail worth keeping, not just "it passed."
2. **`channel_priority`'s enum list needed the SHOUTY_CASE member names,
   not just the lowercase values.** Per §2.3's enum-coercion rule, conda's
   `typify()` tries value-based lookup first, then falls back to name-based
   lookup. For `ChannelPriority`, that means both `"strict"` (value) and
   `"STRICT"` (the member's actual Python identifier) are accepted, but
   `"Strict"` is not. The schema's `enum` array must therefore explicitly
   list `"STRICT"`/`"FLEXIBLE"`/`"DISABLED"` alongside
   `"strict"`/`"flexible"`/`"disabled"`/`true`/`false` — an `enum` cannot
   express "case-sensitive value-or-name lookup" any other way.
3. **`channel_alias`'s "must have a scheme" rule needed a regex, not just a
   textual description.** Modeled as `"pattern": "^$|^[A-Za-z][A-Za-z0-9+.-]*://.*$"`
   (empty string, or an RFC-3986-shaped `scheme://...` prefix), directly
   encoding `channel_alias_validation`'s `has_scheme()` check.
4. **`default_python`'s validation needed a regex approximation of a
   float-range check.** conda's real check (`value[1] == '.'` then
   `2.0 <= float(value) < 4.0`) isn't directly expressible as a JSON Schema
   numeric constraint (the value is a string). Approximated as
   `"pattern": "^$|^[23]\\.[0-9]{1,2}$"` — empty string, or a leading `2`/`3`
   digit, `.`, then one or two more digits — which reproduces the real
   check's actual accept/reject boundary for every practical Python version
   string (`"3.9"`, `"3.11"`, `"2.7"`, etc.) without trying to replicate the
   exact float-truncation quirk (`float("3.10") == 3.1`) bit-for-bit.
5. **§4's catalog intentionally excludes the entire "Conda-build
   Configuration" category** (`Context.category_map`'s 6th group, between
   "Package Linking and Install-time Configuration" and "Output, Prompt,
   and Flow Control Configuration" — hence the numbering gap between §4.5
   and §4.7 above: `bld_path`, `croot` (alias of `_croot`), `anaconda_upload`
   (alias `binstar_upload`), and `conda_build` (alias of `_conda_build`)).
   This is a deliberate scope decision, not an oversight: these four keys
   configure the separate `conda-build` tool (package-building/uploading),
   not `conda` itself, and are out of scope for this `.condarc`-parsing
   research and for `conformance/condarc/**`. Noted here because
   `anaconda_upload` is, mechanically, declared exactly like the in-scope
   nullable-bool parameters (`PrimitiveParameter(None, element_type=(bool,
   NoneType))`, same shape as `always_yes`/`report_errors`/
   `show_channel_urls`) and would otherwise look like a boolish-catalog
   omission — it's excluded on purpose, per this category-level scope
   decision, not because it was missed.
6. **`local_repodata_ttl`'s `(bool, int)` coercion path recognizes a
   strictly smaller boolish-string vocabulary than `boolify()`'s own
   `BOOLISH_TRUE`/`BOOLISH_FALSE` tuples — empirically confirmed while
   generating the exhaustive `boolish_values_accept_*` fixture battery in
   `conformance/condarc/valid/`.** Per §2.1's dispatch table, `{bool, int}`
   type hints go through `typify_str_no_hint(str(value))`, **not**
   `boolify()` — and `typify_str_no_hint` matches against `conda.auxlib
   .type_coercion._Regex`'s hand-written patterns
   (`BOOLEAN_TRUE = r'^true$|^yes$|^on$'`,
   `BOOLEAN_FALSE = r'^false$|^no$|^off$'`, both case-insensitive), which is
   a *narrower* set than `boolify()`'s own `BOOLISH_TRUE = ("true", "yes",
   "on", "y")` / `BOOLISH_FALSE = ("false", "off", "n", "no", "non", "none",
   "")`. Concretely: the bare single-letter tokens `"y"`/`"Y"` and `"n"`/`"N"`,
   plus `"non"`, `"none"`/`"NONE"`/`"None"` (any casing — these instead match
   the regex's separate `NONE` pattern and become literal `None`, which
   isn't in `(bool, int)`), and the empty string `""` (matches no regex at
   all, stays a `str`) **are all individually valid `boolish` values for
   every other boolish-typed key in §4 (`always_yes`, `report_errors`,
   `show_channel_urls`, `use_only_tar_bz2`, `ssl_verify`) but make
   `local_repodata_ttl` specifically raise `InvalidTypeError`.** This means
   there is no single string value that is universally accepted by *every*
   boolish-typed `.condarc` key at once for these seven tokens — they're
   real, individually-valid boolish values, just not simultaneously valid
   across the full boolish set the way `"true"/"yes"/"on"/"false"/"no"/"off"`
   (all matched by both `boolify()` and the narrower regex) are. The
   generator script (`scripts/generate_boolish_condarc_fixtures.py`)
   verifies universality empirically against real conda before emitting a
   fixture, so these seven tokens are correctly excluded from
   `conformance/condarc/valid/boolish_values_accept_*.json` (which apply one
   candidate value to *all* boolish keys in a single fixture) rather than
   silently mis-modeled.
7. **Giving a scalar-typed (`PrimitiveParameter`) key a raw YAML/JSON list
   value crashes with an unhandled `AttributeError`, not a clean
   `InvalidTypeError`** — the same *shape* of bug as the root-type crashes
   in §1.1, just one level down the tree. Empirically confirmed for every
   boolish key (e.g. `always_yes: [1, 2, 3]`):
   `AttributeError: 'YamlRawParameter' object has no attribute 'typify'`.
   Cause: `LoadedParameter._typify_data_structure` (`conda/common/
   configuration.py`) checks `isiterable(value)` *before* checking the
   declared `element_type`, and for a `PrimitiveParameter` whose merged
   `.value` unexpectedly turns out to be a raw list (because nothing
   upstream validates the raw shape against `element_type` before this
   point), it takes the "this is a collection of nested `LoadedParameter`s"
   branch and calls `.typify()` on each *raw* list element — but raw list
   elements from YAML parsing are plain `int`/`str`/etc., not
   `LoadedParameter` instances, so `int.typify` doesn't exist and the
   `AttributeError` propagates uncaught out of `Context()`/
   `Configuration.__init__()`, exactly like §1.1's list/scalar-root bugs.
   For a raw dict/mapping value instead (e.g. `always_yes: {}`), the
   equivalent crash doesn't happen — `Mapping`-shaped raw values apparently
   get caught by an earlier, more defensive check and surface as a clean
   `InvalidTypeError` instead. **This crash is specifically an *emptiness*
   thing, not a Mapping-vs-Sequence thing**: an *empty* list (`[]`) or
   *empty* dict (`{}`) does **not** crash either — `_typify_data_structure`'s
   generator-expression (`v.typify(source) for v in value`) simply never
   executes its body when `value` has zero elements, so the function
   returns an empty `tuple`/`frozendict` unchanged, which then fails
   `collect_errors`'s `isinstance` check cleanly (`MultiValidationError`
   wrapping ordinary `InvalidTypeError`s, one per boolish key). Only a
   *non-empty* list/dict actually reaches `v.typify(source)` on a raw,
   non-`LoadedParameter` element and crashes. **Conformance-testing
   consequence**: all four shapes (`[]`, `[1, 2, 3]`, `{}`, `{"a": 1}`) are
   still correctly `invalid/` (the process still exits non-zero either way,
   so `tests/condarc_conformance.rs`'s `Checker::Conda` outcome is
   unaffected), but only the non-empty ones exercise the real conda *crash
   bug* — `conformance/condarc/invalid/boolish_values_reject_array_empty.json`
   /`..._object_empty.json` hit the clean path;
   `..._array_nonempty.json`/`..._object_nonempty.json` hit the crash. Worth
   remembering if this repo ever tries to reproduce conda's exact *error
   message* for such fixtures, rather than just their pass/fail verdict.
8. **A second, distinct crash bug: `LoadedParameter.typify()`'s
   `TypeCoercionError` handler calls `issubclass(element_type, Enum)`
   without checking that `element_type` is actually a single class first**
   (`conda/common/configuration.py`, in `typify()`, right after `except
   TypeCoercionError as e:`). For every boolish key whose `element_type` is
   a *tuple* (`(bool, NoneType)`, `(str, bool)`, `(bool, int)` — i.e. every
   boolish key in §4), if the underlying coercion actually *raises*
   `TypeCoercionError` (as opposed to merely producing a wrongly-typed
   value that fails `collect_errors`'s `isinstance` check afterwards),
   Python's `issubclass(a_tuple, Enum)` immediately raises an unrelated,
   unhandled `TypeError: issubclass() arg 1 must be a class` — masking
   whatever the *real* validation failure was. Empirically, this fires for
   any string value that both (a) isn't one of `boolify()`'s
   `BOOLISH_TRUE`/`BOOLISH_FALSE`/`NULL_STRINGS` tokens, and (b) isn't
   parseable by Python's `complex()` constructor either (`boolify()`'s
   final fallback) — e.g. `"banana"`, `"enabled"`, or any plain English
   word/typo (`"yess"`, `"Truee"`, `"onn"`, `"flase"`, ...) fed to any of
   the four nullable `(bool, NoneType)` keys (`always_yes`, `report_errors`,
   `show_channel_urls`, `use_only_tar_bz2`; `ssl_verify`'s `(str, bool)` is
   unaffected here specifically because it passes `return_string=True`,
   so unparseable strings pass through unchanged instead of raising).
   Conversely, a numeric-*looking* nonsense string that Python's
   `complex()` happily accepts — `"nan"`, `"1_000"` (PEP 515 underscore
   separators are valid `complex()`/`float()`/`int()` literal syntax) —
   does **not** trigger this crash; `boolify()` returns a plain `True`/
   `False` for it without ever raising, and the fixture's actual rejection
   (if any) comes from a different, cleaner code path instead (e.g.
   `local_repodata_ttl`'s narrower regex leaving it as an unmatched `str`).
   **The crash-vs-clean-rejection boundary for an arbitrary invalid string,
   fed to all boolish keys at once, is therefore exactly: is this string
   parseable by Python's `complex()` builtin?** If no → crash (this item).
   If yes → clean `InvalidTypeError`/`MultiValidationError` from whichever
   boolish key's *own* narrower rules reject it (item 9 below, or
   `local_repodata_ttl`'s regex gap from item 6).
9. **A third, distinct crash bug: `_Regex.HEX`/`.OCT`/`.BIN`
   (`conda/auxlib/type_coercion.py`) store the *builtin* `hex`/`oct`/`bin`
   functions as their regex-match "typish" converters, backwards from what
   `typify_str_no_hint` actually needs.** `_Regex._convert` calls
   `typish(value_string)` when the associated compiled regex matches — for
   `HEX` (`^[-+]?0[xX][0-9a-fA-F]+$`), `OCT`, and `BIN`, `typish` is the
   builtin `hex`/`oct`/`bin` function, which converts an **int to its
   string representation** (`hex(26) == '0x1a'`), not the reverse. So a
   *string* that looks like a hex/octal/binary literal and matches the
   regex — e.g. `local_repodata_ttl: "0x1A"` — calls `hex("0x1A")`, and
   Python's builtin `hex()` requires an actual `int` argument, raising an
   unhandled `TypeError: 'str' object cannot be interpreted as an integer`
   (note: this is a *different* `TypeError` message than item 8's
   `issubclass()` crash, and comes from a different call site —
   `_Regex._convert`'s generator expression, not `LoadedParameter.typify`'s
   exception handler). Confirmed via `local_repodata_ttl: "0x1A"` alone (no
   other keys needed): the traceback bottoms out in
   `_convert`'s `typish(value_string)`. In the full six-key
   `boolish_values_reject_string_hex_literal.json` fixture this fires
   *before* item 8's crash would, because `Context`'s parameters are
   iterated in class-body declaration order during `validate_all()`
   (`conda/base/context.py` declares `local_repodata_ttl` well before
   `always_yes`), so whichever boolish key comes first in that order and
   chokes on a given input determines which of the two crash messages (or
   the item-6/item-7 clean rejections) actually surfaces for a shared
   six-key fixture — a subtlety worth remembering if a future contributor
   tries to assert on conda's exact error *text* rather than just its
   pass/fail verdict.
10. **Whitespace-padded boolish string tokens are *valid*, not invalid** —
    empirically confirmed while building the invalid-side fixture battery
    (`conformance/condarc/invalid/boolish_values_reject_*.json`) that a
    plausible-looking rejection candidate, `" true "`
    (leading/trailing space) or even `"\t\nyes\n\t"` (tabs/newlines), is
    actually accepted and coerces correctly. Cause: `typify()`
    (`conda/auxlib/type_coercion.py`) unconditionally does `value =
    value.strip()` at its very top for any `str` input, before ever
    dispatching on `type_hint` — and the one special case that *skips*
    `typify()` entirely to preserve whitespace
    (`_typify_data_structure`'s `isinstance(value, str) and
    issubclass(type_hint, str)` check, `conda/common/configuration.py`)
    only applies when `type_hint` is the single, exact `str` type, which
    none of the six boolish keys use (they're all tuples). So these two
    values were moved to
    `conformance/condarc/valid/boolish_values_accept_string_whitespace_padded*.json`
    instead of the invalid battery — see `scripts/
    generate_boolish_condarc_fixtures.py`.
11. **`local_repodata_ttl` was pulled out of the shared six-key boolish
    battery entirely and given its own dedicated, single-key fixture
    battery** (`conformance/condarc/valid/local_repodata_ttl_accept_*.json`
    / `conformance/condarc/invalid/local_repodata_ttl_reject_*.json`, via
    `scripts/generate_local_repodata_ttl_fixtures.py` /
    `generate_local_repodata_ttl_reject_fixtures.py`). The shared battery
    (now five keys: `always_yes`, `report_errors`, `show_channel_urls`,
    `use_only_tar_bz2`, `ssl_verify`) applies one candidate value to
    *every* key in the set at once and asks "is the combined document
    valid" — which conflates two different questions when one key in the
    set uses a fundamentally different coercion mechanism than the rest.
    Concretely: `"y"`, `"n"`, `""`, `1.5`, `"1_000"`, `"1.0"`, `"1+2j"`,
    and a bare `null` are all **valid, ordinary `boolify()` values** — but
    they were previously showing up in `conformance/condarc/invalid/
    boolish_values_reject_*.json` purely because `local_repodata_ttl`
    doesn't call `boolify()` at all (it uses `typify_str_no_hint`, a
    separate, narrower hand-rolled regex table — item 6 above), and
    `Context.validate_all()` rejects the *whole* document if *any* key in
    it is invalid. That made "is this value boolish-valid" and "is this
    value valid *specifically for the one key that behaves differently*"
    look like the same question when they aren't. Isolating
    `local_repodata_ttl` confirmed this precisely — e.g. `{"always_yes":
    "y", "report_errors": "y", "show_channel_urls": "y",
    "use_only_tar_bz2": "y", "ssl_verify": "y"}` (no `local_repodata_ttl`)
    is valid; `{"local_repodata_ttl": "y"}` alone is not. All 8 candidates
    that flipped from "shared-battery-invalid" to "shared-battery-valid"
    once `local_repodata_ttl` was removed (`"y"`, `"n"`, `""`, `"1+2j"`,
    `"1_000"`, `"1.0"`, `1.5`, `null`) were moved into
    `boolish_values_accept_*.json`; the shared invalid battery now holds
    only the 7 candidates still genuinely invalid for at least one of the
    *remaining* five keys (both empty/non-empty collection shapes, the
    arbitrary-word and hex-literal crashes via the nullable keys'
    `issubclass()` bug — item 8 — and the string `"null"` specifically via
    `ssl_verify`'s filesystem-existence check — see the refined item 9
    note below).
    One more nuance surfaced while isolating `ssl_verify`: a **bare `null`
    literal** and the **string `"null"`** behave differently for it, even
    though `ssl_verify` is non-nullable. `boolify(None, return_string=True)`
    first does `str(None).strip().lower()` unconditionally (since `None`
    isn't in `BOOL_COERCEABLE_TYPES`) — `"none"` — which *is* itself a
    `BOOLISH_FALSE` token, so raw `None` round-trips all the way to a real
    Python `False` and passes cleanly. But the *string* `"null"` reaches
    the same `str(value).strip().lower()` call already lowercased as
    `"null"`, which is **not** in `BOOLISH_FALSE` (only `"none"` is — they
    are spelled differently) and isn't `complex()`-parseable either, so
    `boolify()` falls through to its `return_string=True` passthrough and
    leaves it as the literal string `"null"` — which then fails
    `ssl_verify_validation`'s filesystem-existence check (item 9), since
    `"null"` isn't a real path. So `null` (the YAML/JSON literal) is valid
    for `ssl_verify`; `"null"` (the string) is not — a one-character
    spelling difference (`"null"` vs `"none"`) is the entire reason, not
    nullability.
    `local_repodata_ttl`'s own dedicated invalid battery additionally
    distinguishes `"non"` (matches no `_Regex` pattern at all, stays an
    unmatched `str`) from `"none"`/`"null"` (both match `_Regex.NONE` and
    become the actual Python `None`) — two structurally different
    resulting values that both still fail the final `(bool, int)`
    `isinstance` check, kept as separate fixtures since they exercise
    different branches of `typify_str_no_hint`.
12. **The full bool-like `.condarc` key catalog splits into four
    categories, only two of which (`(bool, NoneType)` and `(str, bool)`/
    `(bool, int)`) were originally covered by the fixture batteries
    above.** Re-surveying every `Context` parameter turned up:
    - **Category A** — 37 keys declared with the plain, single, exact
      `bool` element_type (non-nullable): `override_channels_enabled`,
      `add_anaconda_token`, `allow_non_channel_urls`, `no_lock`,
      `repodata_use_zst`, `repodata_use_shards`, `offline`,
      `auto_update_conda`, `force_reinstall`,
      `prefix_data_interoperability`, `allow_softlinks`, `always_copy`,
      `always_softlink`, `rollback_enabled`, `extra_safety_checks`,
      `shortcuts`, `non_admin_enabled`, `separate_format_cache`,
      `auto_activate_base`, `changeps1`, `json`, `notify_outdated_conda`,
      `quiet`, `unsatisfiable_hints`, `envvars_force_uppercase`,
      `allow_cycles`, `allow_conda_downgrades`,
      `add_pip_as_python_dependency`, `debug`, `trace`, `dev`,
      `enable_private_envs`, `force_32bit`, `solver_ignore_timestamps`,
      `register_envs`, `protect_frozen_envs`, `no_plugins`.
    - **Category B** — the original four `(bool, NoneType)` nullable keys
      (`always_yes`, `report_errors`, `show_channel_urls`,
      `use_only_tar_bz2`).
    - **Category C** — `ssl_verify` (`(str, bool)`, custom validation).
    - **Category D** — `local_repodata_ttl` (`(bool, int)`, different
      coercion function entirely — §8 items 6/11).

    Per §2.1's dispatch table, Category A goes through the *exact same*
    `typify()`/`boolify()` call as Category B — `boolify(value)` — just
    with `nullable=False` instead of `nullable=True`. This means nearly
    every universally-valid/invalid token already established for
    Categories B/C/D also applies to Category A, and
    `generate_boolish_condarc_fixtures.py` / `generate_boolish_condarc_
    reject_fixtures.py` were expanded so `KEYS` now spans Categories A + B
    + C together (Category D remains split out into its own dedicated
    battery per item 11). Two refinements fell out of doing this
    empirically rather than just assuming boolify()'s behavior is
    identical regardless of `nullable`:
    - **The `issubclass()` crash bug (item 8) requires a *tuple*
      `element_type`, so it cannot fire for Category A.** Empirically
      confirmed: `debug: "banana"` (Category A) raises a clean
      `CustomValidationError: ... 'banana' cannot be boolified.` — no
      crash — because `LoadedParameter.typify()`'s exception handler calls
      `issubclass(element_type, Enum)`, and `bool` (a single class) is a
      perfectly legal argument to `issubclass()`, unlike a tuple such as
      `(bool, NoneType)`. Every arbitrary-word/hex-literal-shaped invalid
      string still makes the *whole* shared-battery document invalid
      (because it still crashes via whichever tuple-typed key — B or C —
      the fixture also sets), but the *mechanism* differs per key: clean
      rejection for Category A, a genuine crash for Category B/C. This is
      why the shared invalid battery is still safe to widen to include
      Category A: the pass/fail verdict is unaffected, only the
      underlying reason (which this suite doesn't assert on) differs.
    - **`always_softlink` is excluded from the shared "set every key to
      the same value" battery — but only from the battery, not from the
      Category A catalog above.** `always_softlink` and `always_copy` are
      the subject of `Context.post_build_validation()`'s *other*
      cross-field rule (§1.4): both simultaneously truthy → a
      `ValidationError` about mutual exclusivity, wholly unrelated to
      boolify() coercion. Since every truthy candidate in the shared
      battery (`true`, `"yes"`, `1`, non-empty collections, ...) sets
      *every* key in `KEYS` to a truthy value at once, including both
      `always_copy` and `always_softlink` would make every such "accept"
      fixture spuriously fail this unrelated rule. `always_copy` alone
      already exercises the identical plain, non-nullable `bool`
      coercion path, so no boolify()-specific coverage is lost; the
      cross-field behavior itself is separately covered by the
      pre-existing `conformance/condarc/invalid/
      always_copy_and_softlink.json`.
13. **Nullability itself has exactly three distinguishing string tokens,
    not more: `"null"`, `"~"`, and `"\0"`.** `NULL_STRINGS = ("none", "~",
    "null", "\0")` (§2.2) overlaps with `BOOLISH_FALSE` only at `"none"` —
    every other `NULL_STRINGS` member is *not* a `BOOLISH_FALSE` token.
    Concretely, for `boolify(value, nullable=...)`:
    - `nullable=True` (Category B): step 4 fires for any of the three
      non-`"none"` `NULL_STRINGS` tokens before step 5 is ever reached →
      the value becomes Python `None` → the field loads successfully as
      `None`.
    - `nullable=False` (Category A, and — since `ssl_verify` isn't
      nullable either but *is* a `(str, bool)` string-passthrough type —
      excluded from this specific comparison): step 4 is skipped
      entirely (its `nullable and ...` guard short-circuits), so control
      falls straight to step 5 (`BOOLISH_FALSE`, which doesn't contain
      these three), then step 6 (`complex()`, which can't parse any of
      them either) → `TypeCoercionError` → the field fails to load.

    Empirically confirmed for all three tokens against both a nullable key
    (`always_yes`) and a plain Category A key (`debug`):
    `always_yes: "null"` / `"~"` / `"\0"` all load successfully (as
    `None`); `debug: "null"` / `"~"` / `"\0"` all raise
    `CustomValidationError: ... cannot be boolified.` — a clean rejection,
    not a crash, for the same reason as item 12's `issubclass()` note:
    `bool` is a single class, not a tuple, so `LoadedParameter.typify()`'s
    exception handler never hits the `issubclass()` crash for these
    Category A keys.
    Raw JSON/YAML `null` (Python `None`, not the string) is **not** one of
    these three — it's already universally valid for every boolify()-based
    key regardless of nullability (§8 item 11's note: `str(None).lower()
    == "none"`, itself a `BOOLISH_FALSE` token, so it resolves via step 5
    for non-nullable fields just as readily as via step 4 for nullable
    ones).

    New dedicated fixture batteries isolate this precisely:
    `conformance/condarc/valid/nullable_values_accept_*.json` sets *only*
    the four Category B keys to each of the three tokens (all valid);
    `conformance/condarc/invalid/nullable_values_reject_*.json` sets
    *only* the 37 Category A keys to the same three tokens, deliberately
    excluding Category B (all invalid) — see
    `scripts/generate_boolish_condarc_fixtures.py` /
    `generate_boolish_condarc_reject_fixtures.py`.
14. **Plain numeric (`int`/`float`) keys, generated by
    `scripts/generate_numeric_condarc_fixtures.py`.** Following up on
    §4's bool-like coverage, the 13 remaining `Context` parameters
    declared with a bare, non-tuple, non-enum numeric `element_type` (11
    `int`: `repodata_threads`, `fetch_threads`, `default_threads`,
    `remote_max_retries`, `remote_backoff_factor`, `verify_threads`,
    `execute_threads`, `auto_stack`, `verbosity`,
    `unsatisfiable_hints_check_depth`, `number_channel_notices`; 2
    `float`: `remote_connect_timeout_secs`, `remote_read_timeout_secs`)
    were surveyed the same way. `local_repodata_ttl` (`(bool, int)`, a
    tuple — item 6/11 above) is deliberately excluded, as are
    conda-build and CLI-only variables (already excluded from §4's
    catalog entirely). None of these 13 keys declare a custom
    `validation=` callable (§3), so — unlike `default_python`'s
    range check or `ssl_verify`'s filesystem check — there is **no
    semantic "must be non-negative" or "must be in range" enforcement
    anywhere**: empirically confirmed that e.g. `remote_max_retries: -100`
    and `remote_connect_timeout_secs: -1.5` both load successfully. Type
    coercion is a bare `int(value)`/`float(value)` constructor call
    (§2.1's `elif type_hint is not None` branch), with `ValueError`
    caught and turned into a clean `CustomValidationError`. Findings:
    - **`int()` and `float()` accept an overlapping but not identical
      vocabulary.** Every `int()`-valid value is also `float()`-valid
      (plain numbers, bignum strings, `bool`, `+`/`-` signs, leading
      zeros in a string, single-`_`-separated digit groups per PEP 515,
      whitespace-padding). `float()` additionally accepts decimal points
      (`"3.5"`, `".5"`, `"5."`), scientific notation (`"1e3"`, `"1e-3"`),
      and case-insensitive, optionally-signed `"nan"`/`"inf"`/
      `"infinity"` tokens — all silently accepted with **no
      finiteness/NaN rejection** anywhere in `Context`. `int()` rejects
      every one of these with a clean `ValueError`.
    - **`int(value)` truncates toward zero, not floor/round**:
      `int(3.7) == 3` and, notably, `int(-3.7) == -3` (**not** `-4`) —
      empirically confirmed for a JSON/YAML float fed directly to an
      `int`-typed key. A hypothetical implementation that floors instead
      of truncating would silently disagree with real conda on every
      negative non-integer input.
    - **Neither `int()` nor `float()` accepts non-base-10 numeric-base
      *strings*** (`"0x1A"`, `"0o17"`, `"0b101"`) **or a `complex()`-
      shaped string** (`"1+2j"`) — rejected identically by both, despite
      each superficially resembling a valid literal in some other
      language/context. (Separately, an *unquoted* YAML `0x1A` scalar
      *is* natively resolved as a hex int by conda's own YAML loader —
      empirically confirmed — but that path requires a bare/unquoted
      YAML scalar and is unreachable from this suite's JSON-serialized
      fixtures; see `tests/condarc_conformance.rs`'s `check_conda`,
      which always writes fixtures via `serde_json::to_string` first.)
    - **The same two crash bugs already documented for boolish keys
      (items 7/8 above) recur identically here**, since the crash sites
      (`LoadedParameter._typify_data_structure`/`.typify()`) sit upstream
      of the `int()`/`float()` call itself: a non-empty raw JSON
      array/object crashes with an unhandled `AttributeError`; an empty
      one fails cleanly with `InvalidTypeError`; a bare JSON `null`
      crashes with an unhandled `TypeError` (`int()`/`float()` argument
      must be ... not 'NoneType'`) for every one of these 13 keys.
    - **Cross-implementation numeric-storage risk**: real conda (backed
      by arbitrary-precision Python `int`/`float`) accepts integer
      strings far outside `i64`/`u64` range without error (empirically
      confirmed for values straddling `i64::MAX`/`u64::MAX` and beyond).
      More strikingly, a numeral three orders of magnitude past `f64`'s
      ~1.8e308 max finite value, fed to a `float`-typed key, **silently
      overflows to `+inf` with no error raised at all** — yet the
      identical string fed to an `int`-typed key stays an exact,
      arbitrarily-large integer. Any implementation backed by a
      fixed-width numeric type (`i64`, `f64`, JSON Schema's own
      `integer`, ...) risks silently truncating, wrapping, or erroring
      where real conda does neither; `numeric_values_accept_
      *_bignum_*` fixtures exist specifically to catch that divergence.
    Four fixture batteries came out of this: `numeric_values_accept_*`
    (valid for all 13 keys at once), `numeric_values_accept_float_only_*`
    (valid for the 2 `float` keys only), `numeric_values_reject_*`
    (invalid for all 13 keys at once), and `numeric_values_reject_
    int_only_*` (valid for `float` but invalid for the 11 `int` keys,
    applied to the `int` keys only).
