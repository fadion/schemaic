#!/usr/bin/env bash
#
# Shared helpers for the repository builders. Sourced, never run.
#
# One definition because the thing it guards is `rm -rf` on a path a human
# typed: three copies of a refusal are three chances for one of them to be the
# unguarded one, which is exactly the state this file was written to end.

# The marker a builder drops in every directory it owns.
#
# It is what makes "this is mine to delete" a fact about the directory rather
# than a guess about the path. `packaging/repo/README.md` tells a developer to
# run these builders by hand with an output path of their choosing, so the path
# really is arbitrary input.
SCHEMAIC_DIR_STAMP=".schemaic-build"

# Empty `$dir` for a fresh build, refusing anything that is not demonstrably
# ours to empty.
#
# **Why a refusal and not a prompt.** These run unattended in CI and by hand from
# a README, and the failure mode is not "the build is wrong" — it is a
# developer's home directory or working tree gone, recursively, before a single
# package has been downloaded. `scripts/prune-target.ps1` already makes the same
# call for the same reason ("Never delete anything in a directory that is not
# demonstrably a cargo target dir"); it landed six days from these builders and
# only one of the two was guarded, while the unguarded one is the one a human
# types a path into.
#
# Four things are refused, in increasing order of how much they would cost:
#
#   * an empty argument, which `rm -rf ""` would make `rm -rf .` under some
#     shells' word splitting;
#   * `/` and `$HOME`;
#   * the repository root, or any ancestor of it — a completed path one
#     component short of the intended one lands here;
#   * an existing, non-empty directory with no `SCHEMAIC_DIR_STAMP` in it.
#
# A path that does not exist yet is created, which is CI's case and needs no
# marker. A path that exists and is empty is adopted.
reset_dir() {
    local dir="$1" root="${2:-}"
    if [ -z "$dir" ]; then
        echo "refusing to empty an unnamed directory" >&2
        return 1
    fi
    if [ -e "$dir" ] && [ ! -d "$dir" ]; then
        echo "refusing to empty ${dir}: it is not a directory" >&2
        return 1
    fi

    local abs
    if [ -d "$dir" ]; then
        abs="$(cd "$dir" && pwd -P)"
    else
        local parent
        parent="$(dirname "$dir")"
        [ -d "$parent" ] || mkdir -p "$parent"
        abs="$(cd "$parent" && pwd -P)/$(basename "$dir")"
    fi

    if [ "$abs" = "/" ] || [ "$abs" = "${HOME:-/nonexistent}" ]; then
        echo "refusing to empty ${abs}" >&2
        return 1
    fi
    # An ancestor of the repository, or the repository itself. `${root}/` so
    # `/src/schemaic-notes` is not read as an ancestor of `/src/schemaic`.
    if [ -n "$root" ] && { [ "$abs" = "$root" ] || case "${root}/" in "${abs}/"*) true ;; *) false ;; esac; }; then
        echo "refusing to empty ${abs}: it holds the repository at ${root}" >&2
        return 1
    fi

    if [ -d "$abs" ] && [ ! -f "${abs}/${SCHEMAIC_DIR_STAMP}" ] \
        && [ -n "$(ls -A "$abs" 2>/dev/null)" ]; then
        echo "refusing to empty ${abs}: it is not empty and carries no ${SCHEMAIC_DIR_STAMP}," >&2
        echo "so it was not written by these builders. Point at a new directory, or" >&2
        echo "remove it yourself if you meant this one." >&2
        return 1
    fi

    rm -rf "$abs"
    mkdir -p "$abs"
    : > "${abs}/${SCHEMAIC_DIR_STAMP}"
}
