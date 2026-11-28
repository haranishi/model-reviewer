# 設計：ダウンロードして使える配布版

## 全体
```
tag v0.1.0 ─▶ release.yml ─▶ macos / windows / linux の組み立て（中身の確認と起動の試し）
                          └─▶ release ジョブ：SHA-256 の一覧、下書きの Release（タグのときだけ）
PR（配布に関わるファイル）─▶ 組み立てと確認だけ（Release は作らない）
```

## 決めたこと
- **1つの workflow（`.github/workflows/release.yml`）**。起動はタグ `v*` の push、配布に関わるファイルの PR、手動。書き込みの権限（`contents: write`）は release ジョブだけに付ける。
- **macOS**：`aarch64-apple-darwin` と `x86_64-apple-darwin` を組み立て、`lipo` で1つにする。`Model Reviewer.app`（`Info.plist` の版は Cargo の版）を作り、`codesign -s -` で仮の署名をして `ditto` で zip にする。公証はしない。最小の macOS は依存が実際に要る版を確かめて `LSMinimumSystemVersion` に書く。
- **Windows**：x86_64 の release。ダブルクリックで黒いコンソールの窓が出ないようにする（`windows_subsystem`）。コマンドラインと `--script` の動き（終了コードと出力ファイル）は変えない。
- **Linux**：glibc の古い環境でも動くように `ubuntu-22.04` で組み立てる。実行に要るもの（Vulkan か OpenGL、`libxkbcommon`、日本語フォントなど）は起動の手順に書く。
- **ライセンス表記**：`cargo about`（版を固定して CI で入れる）と、`about.toml`・ひな形をリポジトリに置く。作った表記に内蔵フォントのライセンス本文があるかを確かめ、無ければ失敗させる。`deny.toml` の許可と食い違わないようにする。
- **版の確認**：タグの `v` を除いた値と `Cargo.toml` の版が違えば失敗。
- **起動の試し**：組み立てた実行ファイルを、画面を出せる環境があれば `--script` の短い手順（見本を開いて撮る）で動かす。macOS は必須。Windows（ソフトウェアの描画）と Linux（仮想の画面とソフトウェアの Vulkan）は試し、出来なければ理由を記録して続ける。撮った画像は成果物に残す。
- **SHA-256**：`SHA256SUMS` を Release に置く。
- **文書**：`docs/download.md`（OS ごとの入れ方と警告の越え方、試験版の注意）、組み立ての手順は `docs/build.md` へ移す。README はダウンロードの案内を冒頭に置く（`releases/latest` へのリンク）。Release の本文は `docs/release-notes/v0.1.0.md`。

## 壊れやすい所
- universal の `.app` を仮の署名の後に書き換えると起動できなくなる（署名は最後）。
- `cargo about` は依存の許可ライセンスの指定が合わないと失敗する。内蔵フォントの「OFL・UFL」を忘れない。
- zip の作り方で実行の権限が消える（macOS は `ditto`、Linux は `tar`）。
- README を100行以内に保つ（`README.ja.md` はいま100行ちょうど）。
