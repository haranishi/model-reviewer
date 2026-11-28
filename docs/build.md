# Build from source / ソースからの起動

For a ready-to-run binary, follow the [download guide](download.md). 以下はソースからビルドする場合の手順です。

## Build and run

Install Rust with rustup. The repository pins Rust 1.99.0, rustfmt and clippy in `rust-toolchain.toml`. The minimum supported Rust version is 1.95. A GPU and display are required to open the window. The Linux build needs pkg-config and the development packages for Fontconfig, Wayland and libxkbcommon. Install a Japanese font for the interface.

リポジトリの直下で実行します。2行目は台車の見本を開きます。

```sh
cargo run --release --locked -p model-reviewer
cargo run --release --locked -p model-reviewer -- assets/samples/review-sample.glb
```

You can pass a GLB, STL or review JSON on startup. [Regenerate the samples](../assets/samples/README.md).

| Option | Effect | 用途 |
|---|---|---|
| `--no-textures` | Skip image decoding and disable pasting images. | 画像の復号と貼り付けを無効にする |
| `--texture-budget-mib N` | Limit GPU texture memory, including mipmaps. The default is 512 MiB. | GPU に置く画像の合計容量を決める（縮小版込み、既定 512 MiB） |
| `--script steps.json --out results` | Run the steps and write captures and results/report.json. | 自動操作の画像と結果を保存する。終了コードは成功 0、手順の失敗 1 |
| `--log-metrics` | Write performance values to standard error on exit. | 終了時に性能値を標準エラー出力に書く |

Windows の配布版はコンソールを開きません。PowerShell で自動操作の終了を待ってコードを読む例です。

```powershell
$process = Start-Process .\model-reviewer.exe -ArgumentList '--script steps.json --out results' -Wait -PassThru
$process.ExitCode
```

## Check a build

```sh
cargo fmt --all --check
cargo clippy --all-targets --workspace --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked
```

Regular CI checks dependency licenses against `deny.toml`, checks the minimum Rust version, and runs clippy and tests on macOS, Windows and Linux. The [verification guide](verify.md) covers GPU tests and scripted checks.

## Package a download

Distribution CI generates THIRD-PARTY-LICENSES from the locked release dependencies for all four target triples. It adds the original font notices from the locked font crate and checks the full text. Local packaging can reuse the notices artifact from the same revision.

macOS のファイル選択には macOS 11 で導入された UniformTypeIdentifiers を使います。両方の CPU 向けに 11.0 を指定し、実行ファイルの最小 OS を各スライスで照合してから署名します。手元の CPU 用に .app と ZIP を作る例です。

```sh
export CARGO_ENCODED_RUSTFLAGS="$(printf '%s\037%s' "--remap-path-prefix=$PWD=." \
  "--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}/registry/src=crates.io")"
MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --locked -p model-reviewer
python3 scripts/release/package.py macos target/release/model-reviewer THIRD-PARTY-LICENSES target/local-package
```

The verified archive is in `dist/` under the output directory. Add `--keep-unpacked` to keep the checked extraction in a new `unpacked/` directory. The macOS archive name reflects the architectures in the supplied executable.

Distribution PRs and manual runs build the archives and check their extraction. The macOS hosted smoke test must open the bundled GLB and capture it. Windows and Linux attempt the same check on hosted displays and retain diagnostics when rendering is unavailable. A push of the matching version tag additionally attaches the three archives and SHA256SUMS to a draft release. A maintainer publishes the draft.
