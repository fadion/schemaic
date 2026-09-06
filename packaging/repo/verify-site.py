#!/usr/bin/env python3
"""Read a built package site the way apt and dnf would, offline.

Driven by verify-site.sh, which does the signature half with gpgv and rpmkeys
and leaves the index half here, where parsing XML and checksumming files is
honest work rather than a shell pipeline.

What this catches is metadata that disagrees with the packages beside it: a
Packages file naming a .deb that was pruned, a Release whose SHA256 section
still describes the previous run's Packages.gz, a primary.xml pointing at an
rpm that got re-signed after it was indexed. Every tool involved reports
success for all three, and the only place they surface is a user machine
failing to install.
"""

from __future__ import annotations

import gzip
import hashlib
import os
import sys
import xml.etree.ElementTree as ET

COMMON = "{http://linux.duke.edu/metadata/common}"
REPO = "{http://linux.duke.edu/metadata/repo}"

problems: list[str] = []


def digest(path: str, algo: str = "sha256") -> str:
    h = hashlib.new(algo)
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_file(root, rel, want_hash, want_size, algo, what) -> None:
    path = os.path.join(root, rel)
    if not os.path.isfile(path):
        problems.append(f"{what}: {rel} is named by the index but missing")
        return
    actual = os.path.getsize(path)
    if want_size is not None and actual != int(want_size):
        problems.append(f"{what}: {rel} is {actual} bytes, the index says {want_size}")
    if want_hash and digest(path, algo) != want_hash:
        problems.append(f"{what}: {rel} does not match its {algo} in the index")


def check_apt(site: str) -> list[str]:
    deb = os.path.join(site, "deb")
    dist = os.path.join(deb, "dists", "stable")

    # Release lists every file under dists/ with its size and hash, so a stale
    # Packages.gz left beside a fresh Packages shows up here and nowhere else.
    with open(os.path.join(dist, "Release"), encoding="utf-8") as fh:
        release = fh.read()
    in_sha256 = False
    listed = 0
    for line in release.splitlines():
        if not line.startswith(" "):
            in_sha256 = line.startswith("SHA256:")
            continue
        if in_sha256:
            want, size, rel = line.split()
            check_file(dist, rel, want, size, "sha256", "Release")
            listed += 1
    if listed == 0:
        problems.append("Release: no SHA256 section")

    index = os.path.join(dist, "main", "binary-amd64", "Packages")
    with open(index, encoding="utf-8") as fh:
        stanzas = [s for s in fh.read().split("\n\n") if s.strip()]
    if not stanzas:
        problems.append("Packages: empty index")

    versions = []
    for stanza in stanzas:
        fields = {}
        for line in stanza.splitlines():
            if line and not line[0].isspace() and ":" in line:
                key, _, value = line.partition(":")
                fields[key.strip()] = value.strip()
        # apt resolves Filename against the archive URL, so an absolute path or
        # a leading ./ here fetches nothing. It is the classic failure of a
        # hand-rolled repository, and it is invisible until a client tries.
        filename = fields.get("Filename", "")
        if not filename.startswith("pool/"):
            problems.append(
                f"Packages: Filename {filename!r} is not relative to the archive root"
            )
            continue
        check_file(deb, filename, fields.get("SHA256"), fields.get("Size"), "sha256", "Packages")
        versions.append(fields.get("Version", "?"))

    pooled = sum(len(files) for _, _, files in os.walk(os.path.join(deb, "pool")))
    if pooled != len(stanzas):
        problems.append(
            f"Packages: indexes {len(stanzas)} package(s) but the pool holds {pooled}"
        )
    return sorted(set(versions))


def check_rpm(site: str) -> list[str]:
    rpm = os.path.join(site, "rpm")

    repomd = ET.parse(os.path.join(rpm, "repodata", "repomd.xml")).getroot()
    primary_rel = None
    for data in repomd.findall(f"{REPO}data"):
        location = data.find(f"{REPO}location").get("href")
        checksum = data.find(f"{REPO}checksum")
        check_file(rpm, location, checksum.text, None, checksum.get("type"), "repomd")
        if data.get("type") == "primary":
            primary_rel = location
    if primary_rel is None:
        problems.append("repomd: no primary index")
        return []

    with gzip.open(os.path.join(rpm, primary_rel)) as fh:
        primary = ET.parse(fh).getroot()
    packages = primary.findall(f"{COMMON}package")
    if not packages:
        problems.append("primary: empty index")

    versions = []
    for package in packages:
        location = package.find(f"{COMMON}location").get("href")
        checksum = package.find(f"{COMMON}checksum")
        size = package.find(f"{COMMON}size").get("package")
        check_file(rpm, location, checksum.text, size, checksum.get("type"), "primary")
        versions.append(package.find(f"{COMMON}version").get("ver"))
    return sorted(set(versions))


def check_client_config(site: str) -> None:
    keyring = "/usr/share/keyrings/schemaic-archive-keyring.gpg"
    expected = {
        "schemaic.sources": [f"Signed-By: {keyring}", "/deb", "Suites: stable"],
        "schemaic.list": [f"signed-by={keyring}", "/deb", "stable main"],
        # Both checks, not one: gpgcheck covers the signature inside each
        # package and repo_gpgcheck the one over the index.
        "schemaic.repo": ["gpgcheck=1", "repo_gpgcheck=1", "/rpm", "schemaic.asc"],
        "schemaic.asc": ["BEGIN PGP PUBLIC KEY BLOCK"],
        "index.html": ["schemaic.sources", "schemaic.repo"],
    }
    for name, needles in expected.items():
        path = os.path.join(site, name)
        if not os.path.isfile(path):
            problems.append(f"{name} is missing from the site")
            continue
        with open(path, encoding="utf-8", errors="replace") as fh:
            body = fh.read()
        for needle in needles:
            if needle not in body:
                problems.append(f"{name} does not mention {needle!r}")

    # A placeholder that survived means the landing page is telling people to
    # paste a URL that is not a URL.
    with open(os.path.join(site, "index.html"), encoding="utf-8") as fh:
        index = fh.read()
    for placeholder in ("__BASE_URL__", "__VERSION__", "__FINGERPRINT__"):
        if placeholder in index:
            problems.append(f"index.html still contains the {placeholder} placeholder")


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: verify-site.py <site-dir>", file=sys.stderr)
        return 2
    site = argv[1]

    deb_versions = check_apt(site)
    rpm_versions = check_rpm(site)
    check_client_config(site)

    # The two formats are built from the same set of releases, so a mismatch
    # means one of them silently lost a package on the way in.
    if deb_versions != rpm_versions:
        problems.append(
            f"the two repositories disagree: deb has {deb_versions}, rpm has {rpm_versions}"
        )

    print(f"  deb: {len(deb_versions)} version(s) {deb_versions}")
    print(f"  rpm: {len(rpm_versions)} version(s) {rpm_versions}")

    if problems:
        print()
        for problem in problems:
            print(f"  FAILED  {problem}")
        return 1
    print("  ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
