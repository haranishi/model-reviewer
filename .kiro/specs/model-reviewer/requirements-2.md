# Requirements Document（要件 7〜13）

[requirements.md](requirements.md) の続き。用語と範囲は同じ。

### Requirement 7: 視点の保存
**Objective:** As a レビューする人, I want 見せたい視点に名前を付けて残せること, so that 同じ見え方へ何度でも戻れる

#### Acceptance Criteria
1. When 名前を付けて視点の保存が指示された, the 本アプリ shall 現在のカメラ状態をその名前で保存する。
2. When 保存した視点が選ばれた, the 本アプリ shall 保存時と同じ位置・向き・ズームへ戻す。
3. When 視点の削除が指示された, the 本アプリ shall その視点を消す。

### Requirement 8: レビュー状態の保存と復元
**Objective:** As a レビューする人, I want レビューの結果をモデルとは別のファイルに残し、後で同じ状態に戻せること, so that 指摘を失わず、別のモデルへ誤って当てはめない

#### Acceptance Criteria
1. When レビューの保存が指示された, the 本アプリ shall 元のGLBを書き換えずに、別のJSONファイルへ書き出す。
2. The 本アプリ shall JSONに、形式の版・元モデルのパス・識別情報（大きさとハッシュ）・非表示にした部品・注釈（位置、部品、本文）・保存した視点を含める。
3. When レビューJSONが開かれ、元モデルが保存時と同一である, the 本アプリ shall 表示状態・注釈・視点を復元する。
4. If 元モデルの内容が保存時と違う, then the 本アプリ shall 何も復元せず、違うモデルであることと、正しいGLBを指定する方法を伝える。
5. If 元モデルが見つからない, then the 本アプリ shall 探した場所を示し、GLBの指定を求める。
6. When GLBが指定し直された, the 本アプリ shall 識別情報が一致するときだけ復元する。
7. If JSONが壊れている、または本アプリより新しい版である, then the 本アプリ shall 読み込まずに理由を伝える。
8. While 保存していない変更がある, the 本アプリ shall その旨を表示し、変更を失う操作（別のモデルを開く・レビューを読み込む・終了）の前に確認する。

### Requirement 9: 軽量・高速にするための振る舞い
**Objective:** As a 利用者, I want 無駄な処理をしないこと, so that 大きなモデルでも軽く動く

#### Acceptance Criteria
1. The 本アプリ shall フレームごとにモデルの形状データを複製しない。
2. The 本アプリ shall 変更のない頂点・インデックスをGPUへ送り直さない。
3. The 本アプリ shall 同じメッシュを参照する部品の間で、形状データをCPUとGPUの両方で共有する。
4. When モデルが切り替わった, the 本アプリ shall 前のモデルのCPU・GPUリソースを解放する。
5. While 入力も状態の更新もない, the 本アプリ shall 画面を描き直さない。
6. The 本アプリ shall 3Dの描き直しを、カメラ・表示状態・選択・モデル・ビューの大きさが変わったフレームに限る。

### Requirement 10: 性能を確認できること
**Objective:** As a 利用者・開発者, I want 何を測った値かが分かる性能情報, so that 改善の前後を正しく比べられる

#### Acceptance Criteria
1. The 本アプリ shall 詳細パネルに、ファイルサイズ・ノード数・メッシュ数・三角形数を表示する。
2. The 本アプリ shall ファイルの読み取りと解析にかかった時間を、工程ごとに表示する。
3. The 本アプリ shall 読み込み開始から最初にモデルを描いたフレームまでの時間を表示する。
4. While 操作中, the 本アプリ shall フレームごとのCPU処理時間とフレーム間隔を表示する。
5. The 本アプリ shall 各値に、何を測ったかの説明を添える。
6. The 本アプリ shall CPU側で測った時間をGPUの処理時間として表示せず、測っていない値は「未計測」と示す。
7. Where メモリ使用量をOSから取得できる, the 本アプリ shall それを取得元の名前とともに表示する。
8. The 本アプリ shall README に、実測していない比較（「何倍速い」など）を書かない。

### Requirement 11: 画面構成
**Objective:** As a レビューする人, I want 3Dビューが主役の落ち着いた日本語の画面, so that 説明を読まずに使い始められる

#### Acceptance Criteria
1. The 本アプリ shall 上部に操作、左に部品ツリー、中央に3Dビュー、右に注釈と視点、下部に状態を配置する。
2. The 本アプリ shall 左右のパネルの幅を調整でき、折りたためる。
3. The 本アプリ shall 画面の文言を日本語で表示し、日本語の字形のフォントで描く。
4. The 本アプリ shall 詳細な性能情報を、開いたときだけ表示する。
5. The 本アプリ shall 動かない機能のボタンを置かず、いま使えない操作は使えない見た目にする。
6. The 本アプリ shall 状態の区別を色だけに頼らず、文字か形でも示す。

### Requirement 12: サンプルと検証
**Objective:** As a 開発者, I want 利用条件の明らかなサンプルと、人の操作を邪魔しない検証手段, so that 誰でも同じ確認を繰り返せる

#### Acceptance Criteria
1. The リポジトリ shall 複数の部品・変換つきの親子ノード・同じメッシュの共有・部品ごとの色を含むサンプルGLBと、それを生成するRustのプログラム、出典とライセンスの記録を含む。
2. The テスト shall ノード階層の変換の合成、変換された部品への当たり判定、レビューJSONの往復、別モデルへの誤った復元の防止、不正なGLBで落ちないことを確かめる。
3. The リポジトリ shall `cargo fmt --check`・`cargo test`・`cargo clippy --all-targets`・`cargo build --release` が通る。
4. The 本アプリ shall OSの入力を注入せずに、開く→回転・移動・ズーム→選択して非表示→注釈→視点保存→レビュー保存→再起動して復元、を自動で実行し、画面を画像で保存できる。
5. The テスト shall GPUが要るものと要らないものを分け、実行しなかった検証を成功として数えない。
6. When 起動引数でGLBまたはレビューJSONが渡された, the 本アプリ shall 起動時にそれを開く。

### Requirement 13: 公開リポジトリとしての体裁
**Objective:** As a 貢献したい開発者, I want 使い方・範囲・条件が読めば分かること, so that 安心して試し、手を入れられる

#### Acceptance Criteria
1. The README shall セットアップ・起動方法・操作方法・対応GLBの範囲・レビュー保存形式・検証方法を記載する。
2. The リポジトリ shall MIT OR Apache-2.0 のライセンス文書を含み、著作権表記を「model-reviewer contributors」とする。
3. The リポジトリ shall 個人名・メールアドレス・端末固有の情報・秘密の値を含まない。
4. The リポジトリ shall コードと道具をすべてRustで書く。
5. The リポジトリ shall 動作を確認した依存の版を Cargo.lock に記録する。
