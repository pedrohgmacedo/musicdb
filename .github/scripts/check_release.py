# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Check whether Cargo.toml bumped to an unreleased version and find its exact commit."""

import argparse
import os
import subprocess
import sys
import tomllib
from pathlib import Path

ZERO_SHA = "0" * 40


def cargo_pkg(toml_text: str) -> tuple[str, str]:
    pkg = tomllib.loads(toml_text)["package"]
    return pkg["name"], pkg["version"]


def cargo_version_at(rev: str) -> str | None:
    proc = subprocess.run(
        ["git", "show", f"{rev}:Cargo.toml"], capture_output=True, text=True
    )
    return cargo_pkg(proc.stdout)[1] if proc.returncode == 0 else None


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


def github_release_exists(repo: str, tag: str) -> bool:
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


def crates_version_exists(name: str, version: str) -> bool:
    code = subprocess.run(
        [
            "curl", "-sS", "-o", os.devnull, "-w", "%{http_code}",
            "-H", "User-Agent: musicdb-ci",
            f"https://crates.io/api/v1/crates/{name}/{version}",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if code == "200":
        return True
    if code == "404":
        return False
    sys.exit(f"error: crates.io returned HTTP {code} for {name} {version}")


def emit(values: dict[str, str]) -> None:
    lines = "".join(f"{k}={v}\n" for k, v in values.items())
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with Path(out).open("a", encoding="utf-8") as fh:
            fh.write(lines)
    else:
        print(lines, end="")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=("github", "crates"), default="github")
    parser.add_argument("--skip-remote", action="store_true")
    args = parser.parse_args()

    name, version = cargo_pkg(Path("Cargo.toml").read_text(encoding="utf-8"))
    tag = f"v{version}"
    before = os.environ.get("GITHUB_BEFORE", "")

    if os.environ.get("GITHUB_EVENT_NAME") == "push" and before and before != ZERO_SHA:
        if cargo_version_at(before) == version:
            print(f"Cargo.toml version unchanged ({version}); skipping.", file=sys.stderr)
            emit({"should_release": "false"})
            return

    release_sha = find_bump_commit(version)

    if not args.skip_remote:
        if args.target == "github":
            if github_release_exists(os.environ["GITHUB_REPOSITORY"], tag):
                print(f"GitHub Release {tag} already exists; skipping.", file=sys.stderr)
                emit({"should_release": "false"})
                return
        elif crates_version_exists(name, version):
            print(f"{name} {version} is already on crates.io; skipping.", file=sys.stderr)
            emit({"should_release": "false"})
            return

    print(f"Release {tag} ({args.target}) ready from {release_sha}.", file=sys.stderr)
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
