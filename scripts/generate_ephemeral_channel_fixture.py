#!/usr/bin/env python3
"""Generate the checked-in, offline ephemeral-environment conda channel.

The fixture is intentionally assembled with the Python standard library rather
than conda-build: each legacy ``.tar.bz2`` archive contains the conda package
metadata Rattler consumes plus, for ``fixture-probe``, one executable payload.
This keeps the channel tiny, reproducible, and usable through ``file://`` in
integration tests without a network connection or a conda installation.

The generator owns the package and repodata files below
``tests/fixtures/ephemeral_channel``. Its README is checked in separately and
documents the test roles and package names.

Usage:
    python3 scripts/generate_ephemeral_channel_fixture.py
"""

from __future__ import annotations

import hashlib
import io
import json
import shutil
import tarfile
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURE_ROOT = REPO_ROOT / "tests" / "fixtures" / "ephemeral_channel"
BUILD = "0"
BUILD_NUMBER = 0
LICENSE = "BSD-3-Clause"
TIMESTAMP_MS = 1_704_067_200_000
TAR_MTIME = TIMESTAMP_MS // 1_000

UNIX_PROBE = b"#!/bin/sh\nexit 0\n"
WINDOWS_PROBE = b"@exit /b 0\r\n"


@dataclass(frozen=True)
class PackageSpec:
    channel: str
    subdir: str
    name: str
    version: str
    arch: str | None = None
    platform: str | None = None
    noarch: bool = False
    payload_path: str | None = None
    payload: bytes | None = None
    payload_mode: int = 0o644
    corrupt_checksum: bool = False

    @property
    def filename(self) -> str:
        return f"{self.name}-{self.version}-{BUILD}.tar.bz2"


PACKAGES = (
    PackageSpec("", "noarch", "fixture-default-alpha", "1.0.0", noarch=True),
    PackageSpec("", "noarch", "fixture-default-beta", "1.0.0", noarch=True),
    PackageSpec(
        "",
        "noarch",
        "fixture-corrupt-checksum",
        "1.0.0",
        noarch=True,
        corrupt_checksum=True,
    ),
    PackageSpec(
        "",
        "linux-64",
        "fixture-probe",
        "1.0.0",
        arch="x86_64",
        platform="linux",
        payload_path="bin/fixture-probe",
        payload=UNIX_PROBE,
        payload_mode=0o755,
    ),
    PackageSpec(
        "",
        "linux-aarch64",
        "fixture-probe",
        "1.0.0",
        arch="aarch64",
        platform="linux",
        payload_path="bin/fixture-probe",
        payload=UNIX_PROBE,
        payload_mode=0o755,
    ),
    PackageSpec(
        "",
        "osx-arm64",
        "fixture-probe",
        "1.0.0",
        arch="aarch64",
        platform="osx",
        payload_path="bin/fixture-probe",
        payload=UNIX_PROBE,
        payload_mode=0o755,
    ),
    PackageSpec(
        "",
        "win-64",
        "fixture-probe",
        "1.0.0",
        arch="x86_64",
        platform="win",
        payload_path="Scripts/fixture-probe.cmd",
        payload=WINDOWS_PROBE,
    ),
    PackageSpec(
        "",
        "win-arm64",
        "fixture-probe",
        "1.0.0",
        arch="aarch64",
        platform="win",
        payload_path="Scripts/fixture-probe.cmd",
        payload=WINDOWS_PROBE,
    ),
    PackageSpec("priority-a", "noarch", "fixture-priority", "1.0.0", noarch=True),
    PackageSpec("priority-b", "noarch", "fixture-priority", "2.0.0", noarch=True),
)


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def index_json(spec: PackageSpec) -> dict[str, object]:
    index: dict[str, object] = {
        "build": BUILD,
        "build_number": BUILD_NUMBER,
        "depends": [],
        "license": LICENSE,
        "name": spec.name,
        "subdir": spec.subdir,
        "timestamp": TIMESTAMP_MS,
        "version": spec.version,
    }
    if spec.noarch:
        index["noarch"] = "generic"
    else:
        index["arch"] = spec.arch
        index["platform"] = spec.platform
    return index


def paths_json(spec: PackageSpec) -> dict[str, object]:
    if spec.payload_path is None or spec.payload is None:
        paths: list[dict[str, object]] = []
    else:
        paths = [
            {
                "_path": spec.payload_path,
                "path_type": "hardlink",
                "sha256": hashlib.sha256(spec.payload).hexdigest(),
                "size_in_bytes": len(spec.payload),
            }
        ]
    return {"paths": paths, "paths_version": 1}


def add_file(archive: tarfile.TarFile, path: str, content: bytes, mode: int) -> None:
    entry = tarfile.TarInfo(path)
    entry.size = len(content)
    entry.mode = mode
    entry.mtime = TAR_MTIME
    entry.uid = 0
    entry.gid = 0
    entry.uname = ""
    entry.gname = ""
    archive.addfile(entry, io.BytesIO(content))


def build_package(spec: PackageSpec, output_dir: Path) -> dict[str, object]:
    output_dir.mkdir(parents=True, exist_ok=True)
    archive_path = output_dir / spec.filename
    payload_files = [] if spec.payload_path is None else [spec.payload_path]

    with tarfile.open(archive_path, "w:bz2", format=tarfile.GNU_FORMAT) as archive:
        add_file(archive, "info/index.json", json_bytes(index_json(spec)), 0o644)
        add_file(archive, "info/files", ("\n".join(payload_files) + "\n").encode(), 0o644)
        add_file(archive, "info/paths.json", json_bytes(paths_json(spec)), 0o644)
        if spec.payload_path is not None and spec.payload is not None:
            add_file(archive, spec.payload_path, spec.payload, spec.payload_mode)

    archive_bytes = archive_path.read_bytes()
    record = index_json(spec)
    record.update(
        {
            "md5": hashlib.md5(archive_bytes).hexdigest(),
            "sha256": hashlib.sha256(archive_bytes).hexdigest(),
            "size": len(archive_bytes),
        }
    )
    if spec.corrupt_checksum:
        record["md5"] = "0" * 32
        record["sha256"] = "0" * 64
    return record


def write_repodata(channel_dir: Path, subdir: str, records: dict[str, dict[str, object]]) -> None:
    repodata = {"info": {"subdir": subdir}, "packages": records}
    (channel_dir / subdir / "repodata.json").write_bytes(json_bytes(repodata))


def clean_generated_files() -> None:
    for directory in (
        "noarch",
        "linux-64",
        "linux-aarch64",
        "osx-arm64",
        "win-64",
        "win-arm64",
        "priority-a",
        "priority-b",
    ):
        path = FIXTURE_ROOT / directory
        if path.exists():
            shutil.rmtree(path)


def main() -> int:
    FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    clean_generated_files()

    records_by_subdir: dict[tuple[str, str], dict[str, dict[str, object]]] = {}
    for spec in PACKAGES:
        channel_dir = FIXTURE_ROOT / spec.channel
        record = build_package(spec, channel_dir / spec.subdir)
        records_by_subdir.setdefault((spec.channel, spec.subdir), {})[spec.filename] = record
        print(f"WROTE   {(channel_dir / spec.subdir / spec.filename).relative_to(REPO_ROOT)}")

    for (channel, subdir), records in sorted(records_by_subdir.items()):
        channel_dir = FIXTURE_ROOT / channel
        write_repodata(channel_dir, subdir, records)
        print(f"WROTE   {(channel_dir / subdir / 'repodata.json').relative_to(REPO_ROOT)}")

    print(f"\n{len(PACKAGES)} package artifact(s) written.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
