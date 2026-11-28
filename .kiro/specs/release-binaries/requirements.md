# 要件：ダウンロードして使える配布版

## 目的
Rust を入れていない人が、ダウンロードして開くだけで model-reviewer を使えるようにする。いまはソースだけを公開していて、使うには組み立てが要る。

## 要件
1. The 配布 shall GitHub の Releases で、次の3つを配る。macOS 用は Apple Silicon と Intel の両方で動く1つの `.app` を入れた zip、Windows 用は x86_64 の zip、Linux 用は x86_64 の tar.gz。
2. Each 配布物 shall 実行ファイル、`LICENSE-MIT`・`LICENSE-APACHE`、依存ライブラリと内蔵フォントのライセンス表記（`THIRD-PARTY-LICENSES`）、見本の GLB と STL、起動の手順（最初に読む文書）を含む。配布物と一緒に SHA-256 の一覧を置く。
3. The ライセンス表記 shall 使っている依存の版から自動で作り、egui の内蔵フォント（`epaint_default_fonts`）の OFL と Ubuntu Font Licence の本文を含むことを確かめる。許していないライセンスが入ったら組み立てを失敗させる。
4. When タグ `v<版>` を push する, the 自動の組み立て shall 3つの配布物を作り、**下書きの Release** を作る。公開は人が行う。タグの版が `Cargo.toml` の版と違えば失敗させる。
5. When 配布の仕組みを変える PR を出す, the 自動の組み立て shall 配布物を作って中身を確かめるだけにし、Release は作らない。
6. The 配布 shall 有料の署名と公証をしない。初めて開くときに出る OS の警告の越え方を、README と同梱の文書に書く。
7. The 文書 shall Windows と Linux を「画面は未確認の試験版」と明記する。自動の組み立ての仮想の画面で起動と描画を確かめられた場合は、その範囲だけを書き、実機で確かめたとは書かない。
8. The README shall 冒頭を「ひとことの説明 → ダウンロード → 操作の動画」の順にする。英語と日本語の両方を直し、どの .md も100行以内にする（越える内容は `docs/` に分けて入口からリンクする）。
9. The 変更 shall アプリの依存を足さず版も変えない。配布の道具は自動の組み立ての中だけで使い、版を固定する。外部の GitHub Actions は版をコミットの値で固定する。
10. The 配布物と文書 shall 作った人の手元の絶対パス、メール、作業に使った道具の名前を含まない。

## 範囲外
署名と公証、インストーラー（dmg・msi・パッケージ管理への登録）、自動更新、ARM の Windows と Linux。
