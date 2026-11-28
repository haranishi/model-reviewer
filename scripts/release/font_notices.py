"""Emit or check the original notices from the locked egui font package."""

import argparse
import json
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", type=Path, metavar="THIRD-PARTY-LICENSES")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1",
        "--manifest-path", "crates/app/Cargo.toml", "--filter-platform", "aarch64-apple-darwin",
    ]))
    font, = (p for p in metadata["packages"] if p["name"] == "epaint_default_fonts")
    fonts = Path(font["manifest_path"]).parent / "fonts"
    label = f"{font['name']} {font['version']}".encode()
    sections = [b"Embedded fonts from " + label]
    for name in ("OFL.txt", "UFL.txt", "Hack-Regular.txt", "emoji-icon-font-mit-license.txt"):
        body = (fonts / name).read_bytes()
        if not body.strip():
            sys.exit(f"Empty font notice: {name}")
        sections.append(name.encode() + b"\n" + body)
    if args.check:
        notices = args.check.read_bytes()
        credited = any(line.strip() == label for line in notices.splitlines())
        if not credited or any(part not in notices for part in sections):
            sys.exit("Missing crate attribution or incomplete embedded font license")
    else:
        sys.stdout.buffer.write(b"\n".join(sections) + b"\n")


if __name__ == "__main__":
    main()
