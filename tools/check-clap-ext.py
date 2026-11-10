#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Fails when core/clap-ext differs from the oscairn-clap commit core/clap-ext/upstream.json pins.

Only the vendored paths are compared: include/ and LICENSE (the upstream repository also holds
docs and examples, which Oscairn does not vendor). The pinned commit is fetched from the
repository URL in upstream.json, or from OSCAIRN_CLAP_EXT_REPO (a URL or a local clone) when set.
Without the network the check is skipped (exit 77), as the other network tests are, unless
OSCAIRN_REQUIRE_NETWORK is set, which makes that a failure (CI sets it).
"""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

VENDORED = ("include", "LICENSE")
SKIP = 77


def git(*args, cwd=None):
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, timeout=120)


def blob_id(data: bytes) -> str:
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def local_tree(root: Path) -> dict:
    files = {}
    for name in VENDORED:
        path = root / name
        candidates = [path] if path.is_file() else sorted(p for p in path.rglob("*") if p.is_file())
        for p in candidates:
            files[p.relative_to(root).as_posix()] = blob_id(p.read_bytes())
    return files


def main() -> int:
    vendored = Path(__file__).resolve().parent.parent / "core" / "clap-ext"
    pin = json.loads((vendored / "upstream.json").read_text())
    repository, commit = os.environ.get("OSCAIRN_CLAP_EXT_REPO") or pin["repository"], pin["commit"]
    with tempfile.TemporaryDirectory() as temp:
        if git("init", "-q", temp).returncode != 0:
            print("git init failed", file=sys.stderr)
            return 1
        try:
            fetched = git("fetch", "-q", "--depth", "1", repository, commit, cwd=temp)
        except subprocess.TimeoutExpired:
            fetched = None
        if fetched is None or fetched.returncode != 0:
            try:
                reachable = git("ls-remote", "-q", repository, cwd=temp).returncode == 0
            except subprocess.TimeoutExpired:
                reachable = False
            if not reachable:
                print(f"skipped: {repository} is not reachable")
                return 1 if os.environ.get("OSCAIRN_REQUIRE_NETWORK") else SKIP
            print(f"FAIL: {repository} has no commit {commit}" + (f"\n{fetched.stderr.strip()}" if fetched else ""))
            return 1
        listing = git("ls-tree", "-r", "FETCH_HEAD", "--", *VENDORED, cwd=temp)
        if listing.returncode != 0:
            print(listing.stderr, file=sys.stderr)
            return 1
        upstream = {}
        for line in listing.stdout.splitlines():
            meta, path = line.split("\t", 1)
            upstream[path] = meta.split()[2]
    local = local_tree(vendored)
    problems = [f"only in core/clap-ext: {p}" for p in sorted(local.keys() - upstream.keys())]
    problems += [f"missing from core/clap-ext: {p}" for p in sorted(upstream.keys() - local.keys())]
    problems += [f"differs: {p}" for p in sorted(local.keys() & upstream.keys()) if local[p] != upstream[p]]
    for p in problems:
        print(p)
    if problems:
        print(f"FAIL: core/clap-ext is not {repository} at {commit} (see docs/developer-guide.md, 'The CLAP extension headers')")
        return 1
    print(f"ok: core/clap-ext matches {repository} at {commit} ({len(local)} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
