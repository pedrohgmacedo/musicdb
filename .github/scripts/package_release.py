# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Package built release binaries into archives, compute SHA256SUMS.txt, and publish."""

import argparse
import os
import subprocess
import sys
import zipfile
from pathlib import Path

TARGETS = [
    ("aarch64-apple-darwin", "musicdb", "tar.gz"),
    ("aarch64-unknown-linux-gnu", "musicdb", "tar.gz"),
    ("aarch64-unknown-linux-musl", "musicdb", "tar.gz"),
    ("x86_64-apple-darwin", "musicdb", "tar.gz"),
    ("x86_64-pc-windows-msvc", "musicdb.exe", "zip"),
    ("x86_64-unknown-linux-gnu", "musicdb", "tar.gz"),
    ("x86_64-unknown-linux-musl", "musicdb", "tar.gz"),
]


def package_all(tag: str, artifacts_dir: Path, out_dir: Path) -> list[Path]:
    out_dir.mkdir(parents=True, exist_ok=True)
    archives: list[Path] = []

    for target, bin_name, ext in TARGETS:
        bin_path = artifacts_dir / f"bin-{target}" / bin_name
        if not bin_path.is_file() or bin_path.stat().st_size == 0:
            sys.exit(f"error: missing or empty binary: {bin_path}")

        archive = out_dir / f"musicdb-{tag}-{target}.{ext}"
        if ext == "zip":
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
                zf.write(bin_path, arcname=bin_name)
        else:
            bin_path.chmod(0o755)
            subprocess.run(
                ["tar", "-czf", str(archive.resolve()), "-C", str(bin_path.parent), bin_name],
                check=True,
            )
        archives.append(archive)

    sums = subprocess.run(
        ["sha256sum", *(a.name for a in archives)],
        cwd=out_dir,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    checksums = out_dir / "SHA256SUMS.txt"
    checksums.write_text(sums, encoding="utf-8")
    return [*archives, checksums]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--target-sha", required=True)
    parser.add_argument("--artifacts-dir", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--publish", action="store_true")
    args = parser.parse_args()

    assets = package_all(args.tag, args.artifacts_dir, args.out_dir)
    if args.publish:
        cmd = [
            "gh", "release", "create", args.tag,
            "--target", args.target_sha,
            "--title", args.tag,
            "--generate-notes",
        ]
        if repo := os.environ.get("GITHUB_REPOSITORY"):
            cmd.extend(["--repo", repo])
        cmd.extend(str(a) for a in assets)
        subprocess.run(cmd, check=True)


if __name__ == "__main__":
    main()
