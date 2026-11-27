# model-reviewer

A small desktop app, written in Rust, for checking 3D models and making simple changes. Open a self-contained GLB or an STL file, move, rotate and scale parts, add simple shapes, duplicate or delete parts, measure distances, pin notes to surfaces, and export the result as a new GLB. During the session, the app protects the paths of opened models and recognizes their unchanged contents after a rename or move. You can export again to a GLB the app wrote. Review saves can replace an existing file only if the app can read it as a review.

**Status: early version (0.1.0).** The window has been run only on macOS (Apple Silicon). The user interface is in Japanese. Vertices and faces cannot be edited yet.

[日本語の README](README.ja.md)

![Editing the sample model: a cube added to a small cart, with move handles, the parts list and the transform values](docs/images/editor.png)

## Requirements

- Rust via rustup. `rust-toolchain.toml` pins 1.99.0 with rustfmt and clippy; the minimum supported version is 1.95.
- A GPU and a display. On Windows and Linux, CI checks only the build and the automated tests; the window is untested.
- On Linux, install a Japanese font (for example Noto Sans CJK) so the UI text can be shown.

[The step-by-step guide is in Japanese](docs/guide/README.md).

## Run

```
cargo run --release -p model-reviewer
cargo run --release -p model-reviewer -- assets/samples/review-sample.glb
```

Arguments: `[GLB, STL or review JSON]`, `--script` and `--out` for scripted runs, and `--log-metrics` to print performance values on exit. The sample models (a GLB and a 20 x 30 x 40 mm STL box) were generated for this project ([how to regenerate it](assets/samples/README.md)).

## What it does

- **View**: floor grid and axes, an orientation cube, perspective or orthographic projection, and four display modes (shaded, shaded with edges, wireframe, see-through).
- **Move**: select a part, then drag a handle or type values in the transform panel. Move and rotate follow the floor axes; scale follows the part's own axes. Undo and redo.
- **Add and remove**: add a box, sphere, cylinder or plane and set its dimensions (in mm, for example) in the Dimensions panel; selecting a part also shows its overall width, height and depth; duplicate or delete a part together with its children.
- **Measure**: click two points to get the distance and the X, Y and Z differences. The pointer snaps to corners and edges (X turns it off). Measurements are not saved.
- **Section**: cut the model with one plane perpendicular to X, Y or Z to look inside. Drag the arrow or type the position.
- Use the status bar to choose **display units** from file units, mm, cm and m. GLB uses metres. STL opens in mm with Z up. You can change either STL setting in the status bar if the file still matches what you opened.
- **Review**: save notes, hidden parts and named views to a separate review JSON file.
- **Export**: write a new GLB that keeps the original contents (textures, animations, unknown extensions) and changes only what you edited.

## Controls

| Action | Mouse / trackpad | Key |
|---|---|---|
| Orbit | Left drag | |
| Pan | Right drag, or Shift+drag | |
| Zoom | Wheel, two-finger scroll, pinch | |
| Select | Click | Esc to clear |
| Frame all / frame selection / home view | Toolbar buttons | A / F / Home |
| Front, right, top view | Click a face of the cube | 1 / 3 / 7 |
| Projection / display mode | Buttons above the view | 5 / Z, Shift+Z |
| Tools | Column on the left | V select, G move, R rotate, S scale, M measure, N note |
| Undo / redo | Toolbar buttons | Cmd+Z / Shift+Cmd+Z |
| Duplicate / delete | | Cmd+D / Delete (Backspace) |
| Hide / show all | Eye icon, button | H / Shift+H |
| Snapping on / off (measure tool) | Button next to the tool column | X |
| Section view | Button above the view | C |

You can change shortcuts in the app. See [guide 09](docs/guide/09-shortcuts.md). No middle button is needed. On Windows and Linux use Ctrl instead of Cmd. Single-letter keys are ignored while you type in a text field.

## Supported GLB, STL and export

The app reads GLB files whose geometry is inside the file (BIN chunk), with uncompressed triangle meshes. Textures, vertex colors and animations are accepted but shown in a simplified way. **Deleting a part only removes it from the scene: its geometry data stays in the exported file** (the export result tells you how many). Details: [docs/glb.md](docs/glb.md) (Japanese). Binary and ASCII STL files open as one part; exporting one writes a GLB that reads in metres (the app never writes STL). Details: [docs/stl.md](docs/stl.md) (Japanese).

## Review JSON

Reviews store hidden parts, notes, views, and the path and fingerprint of the source model. Restoring a review requires a model with the same size and BLAKE3 hash. STL reviews also store the unit and up axis. A missing unit defaults to mm and a missing up axis defaults to Y. Review saving pauses while a replacement model loads. **`path` is an absolute path and may contain your user name. Check the file before sharing it.** [Review JSON format (Japanese)](docs/review-json.md)

## Limitations

- You can start with an empty document and change a part’s base color. Vertex, edge and face editing, reparenting, multi-selection, texture and metallic material editing, angle measurement and auto-save are unavailable.
- Snapping only looks at the part under the pointer. The section view has a single axis-aligned plane; cut surfaces get a flat colour without caps or outlines, and open or inside-out meshes can show that colour elsewhere.
- For animated parts, playback tools use the animation values instead of your transform. glTF ignores the node transform of skinned parts. The export result counts both.
- One dark theme. Window layout and unit choice are not remembered.
- The unsaved-changes prompt appears on window close, Cmd+Q and the Quit menu, but not on Dock Quit or log out.
- Memory used during export is not returned to the OS right away. GPU time is not measured.

## Development

```
cargo fmt --all --check
cargo test --workspace
cargo clippy --all-targets --workspace -- -D warnings
cargo build --release
```

CI checks formatting, dependency licenses (`cargo deny`), the minimum Rust version, and clippy and tests on macOS, Linux and Windows. It does not run GPU tests or drive the real window; see [docs/verify.md](docs/verify.md). Design documents live in `.kiro/specs/` and `docs/` (Japanese). To contribute, read [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT license](LICENSE-MIT), at your option. The sample model is under the same terms.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

Dependencies keep their own licenses, checked against [deny.toml](deny.toml). The Latin and emoji fonts that egui embeds are under OFL-1.1 and the Ubuntu Font Licence. Anyone who distributes built binaries must include these notices; this repository does not ship binaries yet.
