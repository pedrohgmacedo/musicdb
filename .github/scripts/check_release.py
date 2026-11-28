# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Check whether Cargo.toml bumped to an unreleased version and find its exact commit."""

import os
import subprocess
import sys
import tomllib
from pathlib import Path

ZERO_SHA = "0" * 40


def cargo_version(toml_text: str) -> str:
    return tomllib.loads(toml_text)["package"]["version"]


def cargo_version_at(rev: str) -> str | None:
    proc = subprocess.run(
        ["git", "show", f"{rev}:Cargo.toml"], capture_output=True, text=True
    )
    return cargo_version(proc.stdout) if proc.returncode == 0 else None


def find_bump_commit(version: str) -> str:
    """Return the oldest contiguous commit touching Cargo.toml that set `version`."""
    shas = subprocess.run(
        ["git", "log", "--format=%H", "HEAD", "--", "Cargo.toml"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.splitlines()

    bump_sha = None
    for sha in shas:
        if cargo_version_at(sha) == version:
            bump_sha = sha
        elif bump_sha:
            break
    if not bump_sha:
        sys.exit(f"error: no commit found setting Cargo.toml version to {version}")
    return bump_sha


def release_exists(repo: str, tag: str) -> bool:
    proc = subprocess.run(
        ["gh", "api", f"repos/{repo}/releases/tags/{tag}"],
        capture_output=True,
        text=True,
    )
    if proc.returncode == 0:
        return True
    if "404" in proc.stdout or "404" in proc.stderr:
        return False
    sys.exit(f"error: gh api failed ({proc.returncode}): {proc.stderr.strip()}")


def emit(values: dict[str, str]) -> None:
    lines = "".join(f"{k}={v}\n" for k, v in values.items())
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with Path(out).open("a", encoding="utf-8") as fh:
            fh.write(lines)
    else:
        print(lines, end="")


def main() -> None:
    version = cargo_version(Path("Cargo.toml").read_text(encoding="utf-8"))
    tag = f"v{version}"
    before = os.environ.get("GITHUB_BEFORE", "")

    if os.environ.get("GITHUB_EVENT_NAME") == "push" and before and before != ZERO_SHA:
        if cargo_version_at(before) == version:
            print(f"Cargo.toml version unchanged ({version}); skipping.", file=sys.stderr)
            emit({"should_release": "false"})
            return

    release_sha = find_bump_commit(version)

    if "--skip-github" not in sys.argv:
        repo = os.environ["GITHUB_REPOSITORY"]
        if release_exists(repo, tag):
            print(f"GitHub Release {tag} already exists; skipping.", file=sys.stderr)
            emit({"should_release": "false"})
            return

    print(f"Release {tag} ready from {release_sha}.", file=sys.stderr)
    emit(
        {
            "should_release": "true",
            "version": version,
            "tag": tag,
            "release_sha": release_sha,
        }
    )


if __name__ == "__main__":
    main()
