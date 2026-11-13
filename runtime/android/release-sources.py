"""Collect Ubuntu source packages for the exact binary inputs of the local APK."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
from pathlib import Path
import shutil
import subprocess
import tarfile
from urllib.parse import quote

from lib.rtbuild import base_status, parse_stanzas, read_deb


def ubuntu_sources(base, debs):
    packages = {entry["Package"]: entry for entry in base_status(str(base))}
    for deb in debs:
        control, _ = read_deb(deb)
        with tarfile.open(fileobj=io.BytesIO(control)) as archive:
            entry, = parse_stanzas(archive.extractfile("./control").read().decode())
        packages[entry["Package"]] = entry

    sources = set()
    for package, entry in packages.items():
        name, *source_version = entry.get("Source", package).split()
        version = source_version[0].strip("()") if source_version else entry["Version"]
        sources.add((name, version))
    return sorted(sources)


def download(url, destination, checksum=None, fallback=None):
    if checksum and destination.is_file():
        with destination.open("rb") as source:
            if hashlib.file_digest(source, "sha256").hexdigest() == checksum:
                return
    temporary = destination.with_suffix(destination.suffix + ".part")
    try:
        command = ["curl", "--fail", "--location", "--silent", "--show-error", "--retry", "1",
                   "--connect-timeout", "15", "--speed-limit", "10000", "--speed-time", "45",
                   "--max-time", "300", "--output", temporary]
        result = subprocess.run([*command, url], check=fallback is None)
        if result.returncode:
            subprocess.run([*command, fallback], check=True)
        if checksum:
            with temporary.open("rb") as source:
                if hashlib.file_digest(source, "sha256").hexdigest() != checksum:
                    raise ValueError(f"SHA-256 mismatch for {url}")
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)


def download_ubuntu_source(package, version, directory, output):
    origin = f"https://launchpad.net/ubuntu/+archive/primary/+sourcefiles/{package}/{quote(version)}"
    dsc = f"{package}_{version.split(':')[-1]}.dsc"
    directory.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True, exist_ok=True)
    descriptor = directory / dsc
    if not descriptor.is_file():
        download(f"{origin}/{quote(dsc)}", descriptor)
    fields = descriptor.read_text()
    if f"\nSource: {package}\n" not in fields or f"\nVersion: {version}\n" not in fields:
        raise ValueError(f"Unexpected source package or version in {dsc}")
    checksums = fields.split("\nChecksums-Sha256:\n", 1)[1]
    files = [dsc]
    pool = package[:4] if package.startswith("lib") else package[0]
    archive = f"https://archive.ubuntu.com/ubuntu/pool/main/{pool}/{package}"
    for line in checksums.splitlines():
        if not line.startswith(" "):
            break
        checksum, _, filename = line.split()
        if Path(filename).name != filename:
            raise ValueError(f"Unexpected filename in {dsc}: {filename}")
        download(f"{archive}/{quote(filename)}", directory / filename, checksum,
                 fallback=f"{origin}/{quote(filename)}")
        files.append(filename)
    if len(files) == 1:
        raise ValueError(f"No source files listed in {dsc}")
    for filename in files:
        shutil.copyfile(directory / filename, output / filename)
    return dsc


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, type=Path)
    parser.add_argument("--deb", action="append", default=[])
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    sources = ubuntu_sources(args.base, args.deb)
    with ThreadPoolExecutor(max_workers=8) as pool:
        downloads = []
        for package, version in sources:
            path = Path(package) / version.replace(":", "_")
            downloads.append(pool.submit(download_ubuntu_source, package, version, args.cache / path, args.out / path))
        for download in downloads:
            print(download.result(), flush=True)


if __name__ == "__main__":
    main()
