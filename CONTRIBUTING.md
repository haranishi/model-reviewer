# Contributing / 貢献

Contributions are welcome, in English or Japanese. Maintainer time is limited: replies may be slow, and not every proposal can be accepted.

- **Good first contributions**: reports from Linux or Windows (does the window start, is text shown), bug reports with a small model that reproduces the problem (do not attach private models), documentation and English translation of the user interface or docs.
- **Before a large change**, open an issue and say who it helps and which crate it belongs to ([docs/roadmap.md](docs/roadmap.md)).
- **Checks**: run the four commands in [README.md](README.md). If you skipped one, say so.
- **License**: unless you state otherwise, your contribution is dual licensed under MIT OR Apache-2.0, without any additional terms. No signed agreement is needed.
- **Do not commit** personal names, email addresses, absolute home paths, secrets, external models or fonts.

以下は日本語の詳しい説明です。

## 条件

提出した変更は、MIT OR Apache-2.0 のどちらでも使えるものとして受け取ります。署名つきの同意書は要りません。著作権表記は model-reviewer contributors のままにします。

## 読むところ

使い方と検証は [README.md](README.md) です。判断の理由と、捨てた案は `.kiro/specs/model-reviewer/`（初版）と `.kiro/specs/editor-foundation/`（編集の土台）にあります。提案の前に、触る範囲の文書を読んでください。この先は [docs/roadmap.md](docs/roadmap.md) です。時期は書いてありません。

ロードマップに無い提案には、誰の何を解くかと、下の境界のどこに収まるかを添えてください。

## 境界

依存の向きは `app → render / gltf / core`、`render → core`、`gltf → core` です。`core` に wgpu、egui、ファイル形式のクレートを入れません。

release の panic を abort にしません。読み込みスレッドの catch_unwind が効かなくなります。ネットワークへ送る処理と、そのためのクレートは足しません。「開く」で開いた GLB は書き換えません。書き出しは、元のGLBの中身を保ち、既存の番号を変えない方式です（[docs/glb.md](docs/glb.md)）。この保証を崩す変更は、先に設計を見直してください。

レビューJSONの項目の意味、ノード順の規則、座標系、wgpu と egui の版の組み合わせを変えるときは、先に設計を見直してください。

## 検証

README の4コマンドを通してください。GPUがある環境では `cargo test --workspace -- --include-ignored` も実行します。実行しなかった検証を、成功として数えないでください。手順は [docs/verify.md](docs/verify.md) です。

自動操作は OS の入力へ注入しません。起動の確認は `crates/app/e2e/smoke.json`、開く操作から保存・再起動・復元までの確認は `flow-1.json` と `flow-2.json` を順に使います。画面に関わる変更では、この2つを実機で通してください。編集・書き出し・計測に関わる変更では `edit-1`〜`edit-3` と `measure-1` も通し、撮った画像を開いて確かめてください。

## リポジトリに置かないもの

個人名、メールアドレス、ホームディレクトリの絶対パス、秘密の値、外部のモデル、フォントは入れません。レビューJSONの例を足すときは、絶対パスにユーザー名が無いか確かめてください。

依存を足すときは `Cargo.lock` を更新します。オフラインでは、lock に無い依存を足しません。足す必要が出たら、作業を止めて相談してください。

サンプル `assets/samples/review-sample.glb` を変えるときは、生成器の出力で置き、`assets/samples/README.md` の数と合わせてください。
