"""生成した target/tex の素材で実機の3手順を動かし、各手順の出力を照合する。"""

import hashlib
import json
import subprocess
import sys
import unittest
from pathlib import Path


def run_script(name):
    executable = "model-reviewer.exe" if sys.platform == "win32" else "model-reviewer"
    script = Path(__file__).with_name(f"{name}.json")
    out = Path("target/e2e") / name
    subprocess.run(
        [Path("target/release") / executable, "--script", script, "--out", out],
        check=True,
    )
    return out


def read_state(out, suffix):
    return json.loads((out / f"{out.name}-{suffix}.json").read_text(encoding="utf-8"))


class TextureRuns(unittest.TestCase):
    def look(self, out, suffix, part, mapping, size):
        state = read_state(out, suffix)
        self.assertEqual(state["selection"], part, suffix)
        self.assertEqual(state["textures"]["total"], 1, suffix)
        self.assertEqual(state["textures"]["displayed"], 1, suffix)
        selected = state["selection_texture"]
        self.assertEqual(selected["total"], 1, suffix)
        (draw,) = selected["draws"]
        self.assertEqual(draw["map"], mapping, suffix)
        if size is None:
            self.assertIsNone(draw["size"], suffix)
        else:
            self.assertAlmostEqual(draw["size"], size, delta=size * 1e-5, msg=suffix)
        self.assertRegex(draw["hash"], r"^[0-9a-f]{16}$", suffix)
        color = state["selection_color"]
        self.assertEqual(color["srgb"], "FFFFFF", suffix)
        return draw["hash"], color

    def test_tex_1(self):
        out = run_script("tex-1")
        shown = read_state(out, "shown")["textures"]
        hidden = read_state(out, "hidden")["textures"]
        self.assertIs(shown["shown"], True)
        self.assertEqual((shown["total"], shown["displayed"]), (4, 4))
        self.assertEqual((shown["shrunk"], shown["failed"]), (0, 0))
        self.assertGreater(shown["bytes"], 0)
        self.assertEqual(hidden, {**shown, "shown": False})
        metrics = json.loads((out / "report.json").read_text(encoding="utf-8"))["metrics"]
        first_draw = metrics["first_present_ms"]["value"]
        last_upload = metrics["texture_present_ms"]["value"]
        self.assertLess(first_draw, last_upload)

    def test_tex_2(self):
        out = run_script("tex-2")
        pasted = self.look(out, "pasted", 0, "box", 1)
        self.assertEqual(self.look(out, "sized", 0, "box", 0.25), pasted)
        self.assertEqual(self.look(out, "copy", 1, "box", 0.25), pasted)
        self.assertEqual(self.look(out, "original-reopened", 0, "uv", None), pasted)
        self.assertEqual(self.look(out, "copy-reopened", 1, "uv", None), pasted)

    def test_tex_3(self):
        inputs = [Path("target/tex") / name for name in ("box.stl", "quadrants.png")]
        for path in inputs:
            digest = hashlib.sha256(path.read_bytes()).digest()
            self.addCleanup(self.check_digest, path, digest)
        out = run_script("tex-3")
        pasted = self.look(out, "pasted", 0, "box", 40)
        self.assertEqual(self.look(out, "reopened", 0, "uv", None), pasted)

    def check_digest(self, path, before):
        self.assertEqual(hashlib.sha256(path.read_bytes()).digest(), before, str(path))


if __name__ == "__main__":
    unittest.main()
