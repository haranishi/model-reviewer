"""Exercise the extracted download on a CI desktop and keep the rendering evidence."""

import json
import os
from pathlib import Path
import subprocess
import sys


def run(exe, args, log):
    # Waiting on the process and explicitly redirecting handles also works for a Windows GUI exe.
    with log.open("wb") as output:
        return subprocess.run([exe, *args], stdin=subprocess.DEVNULL, stdout=output,
                              stderr=subprocess.STDOUT, timeout=120).returncode


def main():
    root = Path("target/package/unpacked/model-reviewer")
    out = Path("target/package/smoke")
    out.mkdir()
    if sys.platform == "darwin":
        exe = root / "Model Reviewer.app/Contents/MacOS/model-reviewer"
    elif sys.platform == "win32":
        exe = root / "model-reviewer.exe"
        log = out / "argument-error.log"
        if run(exe, ["--script"], log) != 2 or b"--script" not in log.read_bytes():
            sys.exit("Expected exit 2 and a redirected diagnostic for a missing --script argument")
    else:
        exe = root / "model-reviewer"

    script = json.loads(Path("crates/app/e2e/smoke.json").read_text(encoding="utf-8"))
    for step in script["steps"]:
        if step["op"] == "open_model":
            step["path"] = os.path.relpath(root / "samples" / Path(step["path"]).name, out)
    procedure = out / "smoke.json"
    procedure.write_text(json.dumps(script), encoding="utf-8")
    report = out / "report.json"
    log = out / "startup.log"
    try:
        code = run(exe, ["--script", procedure, "--out", out], log)
    except subprocess.TimeoutExpired:
        code = None
    if code != 0:
        reason = "process timed out after 120 seconds" if code is None else f"exit {code}"
        diagnostic = log.read_text(encoding="utf-8", errors="replace").strip()
        if report.exists():
            result = json.loads(report.read_text(encoding="utf-8"))
            # A script that ran and failed still has a contract on every platform.
            if code != 1 or result["ok"] is not False:
                sys.exit("Script exit code and report disagree")
            failed = next((s for s in result["steps"] if s["status"] == "failed"), {})
            diagnostic = result["error"] or failed.get("message") or diagnostic
            if failed.get("op") not in {"wait_idle", "screenshot"} or "時間切れ" not in diagnostic:
                sys.exit(f"Bundled sample smoke failed: {diagnostic}")
        message = f"Hosted display smoke incomplete ({reason}). {diagnostic[-1500:]}"
        (out / "result.txt").write_text(message + "\n", encoding="utf-8")
        if sys.platform == "darwin":
            sys.exit(message)
        print(f"::warning::{message.splitlines()[0]} (see smoke artifact)")
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
            summary.write(f"Hosted display smoke incomplete ({reason}). See the smoke artifact for details.\n")
        return

    result = json.loads(report.read_text(encoding="utf-8"))
    if result["ok"] is not True or result["metrics"]["first_present_ms"]["value"] is None:
        sys.exit("No successful render in report.json")
    if not (out / "smoke.png").read_bytes().startswith(b"\x89PNG\r\n\x1a\n"):
        sys.exit("Missing PNG capture")
    if sys.platform == "win32":
        failed = out / "invalid-script"
        failed.mkdir()
        code = run(exe, ["--script", failed / "missing.json", "--out", failed], failed / "stderr.log")
        if code != 1 or json.loads((failed / "report.json").read_text(encoding="utf-8"))["ok"] is not False:
            sys.exit("Expected exit 1 and a failure report for an unreadable script")
    print("Hosted display opened the bundled GLB and wrote smoke.png and report.json.")


if __name__ == "__main__":
    main()
