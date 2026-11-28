# WP-T2 復号・縮小・付帯情報除去の検証

macOS（arm64）で追加した11件を含む724件が成功した。失敗は0件、実行対象外は既存の54件だった。

| 検査 | 結果 |
|---|---|
| `cargo fmt --all --check` | 成功 |
| `cargo clippy --all-targets --workspace --locked -- -D warnings` | 成功 |
| `cargo test --workspace --locked` | 724件成功、0件失敗、54件 ignored |
| `cargo build --release --locked` | Rust 1.99.0 で成功 |
| `cargo +1.95 build --release --locked` | Rust 1.95.0 で成功 |
| `cargo deny --frozen check` | 終了1。既定の勧告DBは読み取り専用でロックできず、作業フォルダにはDBがなかった |
| `cargo deny --frozen check bans licenses sources` | `bans ok, licenses ok, sources ok`。いずれもエラー0件、警告0件 |

## 試験した入力と結果

すべて試験内で生成した。[試験コード](tests/texture.rs)

| 試験名 | 確かめたこと |
|---|---|
| `decodes_png_and_jpeg_to_rgba8` | JPEG の生成、RGB・CMYK JPEG と 8・16 ビット PNG の RGBA8 値 |
| `mips_average_in_linear` | 白黒の平均が sRGB で 188 付近、透明度の平均が 128 |
| `odd_sizes_end_at_1x1` | 奇数辺の端を除き、幅か高さが 1 の画像も 1×1 まで縮小 |
| `skip_drops_top_levels` | 指定した段からの画素とバイト数が全段生成時と一致 |
| `too_large_is_refused_before_decoding` | 長辺 8193 のヘッダーだけで `TooLarge` |
| `oversized_pixel_count_is_refused_from_header` | 画素数の境界と、16 ビット画像が復号時の 160 MiB を超える場合 |
| `broken_image_is_an_error` | 途中で切れた画像、異常な区画長や印、破損した圧縮データを検出 |
| `strip_removes_exif_and_text_keeps_pixels` | 注釈と撮影情報を除去。画素、パレット、透明度、色設定を保持 |
| `strip_removes_segments_between_scans` | 2回の走査の間と直後の注釈を除去。`FF 00`・再開マーカー・画素が一致 |
| `strip_drops_bytes_after_eoi` | JPEG と PNG の終端に付けたデータを除去 |
| `strip_is_idempotent` | 除去を2回繰り返しても同じ圧縮バイト列 |

## 依存の差分

ワークスペースに `crates/texture` を加え、`image` の `jpeg` 機能を有効にした。新クレートの直接依存は `model-reviewer-core` と `image` の2つ。
`Cargo.lock` の差分は新クレートの項目と、`image` の依存に加わった `zune-core`（0.5.3）と `zune-jpeg`（0.5.15）。外部410パッケージの名前・版・入手元・チェックサムはすべて一致した。

`cargo tree -i zune-jpeg --locked` の出力。作業フォルダ内のパスは相対表記にした。

```text
zune-jpeg v0.5.15
└── image v0.25.10
    ├── dify v0.8.0
    │   └── egui_kittest v0.37.0
    │       [dev-dependencies]
    │       └── model-reviewer v0.1.0 (crates/app)
    ├── eframe v0.37.0
    │   ├── egui_kittest v0.37.0 (*)
    │   └── model-reviewer v0.1.0 (crates/app)
    ├── egui_kittest v0.37.0 (*)
    ├── model-reviewer v0.1.0 (crates/app)
    └── model-reviewer-texture v0.1.0 (crates/texture)
```

## アプリへの組み込み

表示は `dimensions` で寸法を集め、`plan_levels` が返した `Some(skip)` の画像を1枚ずつ `decode` する。失敗理由は `TexError::message_ja()` で取得できる。
貼り付けでは、元ファイル保護用の長さとハッシュを先に取る。`sniff` の結果を `strip_metadata` に渡し、返ったバイト列で寸法の検査と復号に成功してから、その列を文書に登録する。

Linux・Windows での実行、アプリと GPU の結合、実行時の最大常駐メモリ、脆弱性勧告の照合は未確認。
