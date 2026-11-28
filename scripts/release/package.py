"""Package a built executable and check an extraction of the archive."""

import argparse
import filecmp
import json
import os
from pathlib import Path
import plistlib
import re
from shutil import copyfile
import struct
import subprocess
from tempfile import TemporaryDirectory
from zipfile import ZIP_DEFLATED, ZipFile


def macos_app(binary, root, version):
    arches = set(subprocess.check_output(["lipo", "-archs", binary], text=True).split())
    load_commands = subprocess.check_output(["otool", "-arch", "all", "-l", binary], text=True)
    minimums = re.findall(r"^\s*minos\s+(\S+)", load_commands, re.MULTILINE)
    if not arches or arches - {"arm64", "x86_64"} or minimums != ["11.0"] * len(arches):
        raise ValueError("Build each macOS slice with MACOSX_DEPLOYMENT_TARGET=11.0")
    app = root / "Model Reviewer.app"
    contents = app / "Contents"
    exe = contents / "MacOS/model-reviewer"
    exe.parent.mkdir(parents=True)
    copyfile(binary, exe)
    exe.chmod(0o755)
    bundle_version = re.match(r"\d+\.\d+\.\d+", version).group()
    with (contents / "Info.plist").open("wb") as plist:
        plistlib.dump({
            "CFBundleExecutable": exe.name,
            "CFBundleIdentifier": "org.model-reviewer.app",
            "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleName": "Model Reviewer",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": bundle_version,
            "CFBundleVersion": bundle_version,
            "LSMinimumSystemVersion": "11.0",
            "NSHighResolutionCapable": True,
        }, plist)
    subprocess.run(["codesign", "--force", "--sign", "-", app], check=True)
    arch = "universal" if len(arches) == 2 else next(iter(arches))
    return exe, f"macos-{arch}"


def check_windows_exe(exe):
    with exe.open("rb") as pe:
        if pe.read(2) != b"MZ":
            raise ValueError("Missing Windows executable header")
        pe.seek(60)
        offset, = struct.unpack("<I", pe.read(4))
        pe.seek(offset)
        if pe.read(6) != b"PE\x00\x00\x64\x86":
            raise ValueError("Expected an x86-64 PE executable")
        pe.seek(offset + 24 + 68)  # Subsystem in the PE optional header.
        if int.from_bytes(pe.read(2), "little") != 2:
            raise ValueError("Expected the Windows GUI subsystem")


def archive_and_extract(system, root, archive, unpacked):
    if system == "macos":
        subprocess.run(["ditto", "-c", "-k", "--norsrc", "--keepParent", root, archive], check=True)
        subprocess.run(["ditto", "-x", "-k", archive, unpacked], check=True)
    elif system == "linux":
        subprocess.run(["tar", "-czf", archive, "-C", root.parent, root.name], check=True)
        subprocess.run(["tar", "-xzf", archive, "-C", unpacked], check=True)
    else:
        with ZipFile(archive, "w", ZIP_DEFLATED) as out:
            for file in sorted(root.rglob("*")):
                if file.is_file():
                    out.write(file, file.relative_to(root.parent))
        with ZipFile(archive) as packed:
            packed.extractall(unpacked)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("system", choices=["macos", "windows", "linux"])
    parser.add_argument("binary", type=Path)
    parser.add_argument("notices", type=Path)
    parser.add_argument("work", type=Path, help="Directory for the verified archives in dist/")
    parser.add_argument("--keep-unpacked", action="store_true", help="Keep the checked tree in a new unpacked/")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1", "--no-deps",
    ]))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "model-reviewer")
    dist = args.work / "dist"
    dist.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(dir=args.work) as temp:
        root = Path(temp) / "model-reviewer"
        unpacked = Path(temp) / "check"
        unpacked.mkdir()
        samples = root / "samples"
        samples.mkdir(parents=True)
        for ext in ("glb", "stl"):
            name = f"review-sample.{ext}"
            copyfile(Path("assets/samples") / name, samples / name)
        for name in ("LICENSE-MIT", "LICENSE-APACHE"):
            copyfile(name, root / name)
        copyfile(args.notices, root / "THIRD-PARTY-LICENSES")
        copyfile("docs/download.md", root / "START-HERE.txt")
        if args.system == "macos":
            exe, platform = macos_app(args.binary, root, version)
        else:
            exe = root / ("model-reviewer.exe" if args.system == "windows" else "model-reviewer")
            copyfile(args.binary, exe)
            exe.chmod(0o755)
            platform = f"{args.system}-x86_64"
        suffix = "tar.gz" if args.system == "linux" else "zip"
        archive = Path(temp) / f"model-reviewer-{version}-{platform}.{suffix}"
        archive_and_extract(args.system, root, archive, unpacked)
        for file in root.rglob("*"):
            unpacked_file = unpacked / file.relative_to(root.parent)
            if file.is_file() and not filecmp.cmp(file, unpacked_file, shallow=False):
                raise ValueError(f"Archive contents differ: {file.name}")
        extracted = unpacked / exe.relative_to(root.parent)
        if args.system == "windows":
            check_windows_exe(extracted)
        elif not os.access(extracted, os.X_OK):
            raise ValueError("Executable permission lost in archive")
        if args.system == "linux":
            with extracted.open("rb") as elf:
                header = elf.read(20)
            if header[:6] != b"\x7fELF\x02\x01" or header[18:20] != b"\x3e\x00":
                raise ValueError("Expected a little-endian x86-64 ELF executable")
        if args.system == "macos":
            app = unpacked / root.name / "Model Reviewer.app"
            subprocess.run(["codesign", "--verify", "--strict", "--all-architectures", app], check=True)
        if args.keep_unpacked:
            unpacked.rename(args.work / "unpacked")
        print(archive.replace(dist / archive.name))


if __name__ == "__main__":
    main()
