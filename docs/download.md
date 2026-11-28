# Model Reviewer — ダウンロードと初回起動 / Download and first launch

GitHub Releases の「Assets」から OS に合うファイルと SHA256SUMS をダウンロードします。Rust は不要です。
Choose your archive and SHA256SUMS under Assets in the latest published release. No Rust installation is needed.
https://github.com/haranishi/model-reviewer/releases/latest

## 展開する前に / Verify the download

ダウンロードしたフォルダーで、使う OS のコマンドを実行します。表示された値が SHA256SUMS の同じファイル名の行と一致することを確かめます。
In the download folder, run the command for your OS. Compare the result with the entry for that filename in SHA256SUMS.

- macOS (Terminal)　`shasum -a 256 model-reviewer-*-macos-universal.zip`
- Windows (PowerShell)　`Get-FileHash .\model-reviewer-*-windows-x86_64.zip -Algorithm SHA256`
- Linux　`sha256sum model-reviewer-*-linux-x86_64.tar.gz`

## macOS — Apple Silicon / Intel

macOS 11 以降向けに組み立てています（画面を確かめたのは Apple Silicon の新しい macOS だけです）。Metal 対応の GPU が必要です。ZIP を展開し、Model Reviewer.app を「アプリケーション」に移して開きます。
公証を受けていないため、開発元を確認できないと表示される場合があります。起動を一度試した後、「システム設定」→「プライバシーとセキュリティ」→「このまま開く」を選びます。macOS 11・12 では「システム環境設定」→「セキュリティとプライバシー」→「一般」にあります。

The app is built for macOS 11 or later (only tested on a recent macOS on Apple Silicon) and needs a Metal-capable GPU. Unzip the download, move Model Reviewer.app to Applications and open it. The app has an ad-hoc signature and is unnotarized. If macOS reports an unidentified developer, try opening it once, then use System Settings → Privacy & Security → Open Anyway. On macOS 11–12, use System Preferences → Security & Privacy → General.
https://support.apple.com/en-us/102445

## Windows — x86-64

ZIP を「すべて展開」し、model-reviewer.exe を開きます。署名がないため、SmartScreen が警告することがあります。配布元とハッシュを確認したら「詳細情報」→「実行」を選びます。組織の管理端末で実行を許可されていない場合は、管理者に確認してください。

Extract all files and open model-reviewer.exe. If SmartScreen warns about the unsigned executable, verify the download, then choose More info → Run anyway when available. A managed PC may require administrator approval.

## Linux — x86-64

Ubuntu 22.04 でビルドする glibc 向けの配布版です。X11 または Wayland の画面、Vulkan または OpenGL 対応のドライバー、libxkbcommon、Fontconfig と日本語フォント（Noto Sans CJK など）が必要です。「開く」はデスクトップのファイル選択機能（XDG Desktop Portal）を使います。下の tar コマンドで展開し、同じ場所から起動してください。

Extract with `tar -xzf model-reviewer-*-linux-x86_64.tar.gz` and run `./model-reviewer/model-reviewer` from the same directory. The build targets Ubuntu 22.04 and compatible glibc systems. Use an X11 or Wayland desktop, Vulkan or OpenGL drivers, libxkbcommon, Fontconfig and a Japanese font such as Noto Sans CJK. The file chooser uses XDG Desktop Portal.

## 見本を開く / Try a sample

「開く」を押し、展開したフォルダーの samples/review-sample.glb を選ぶと台車が出ます。samples/review-sample.stl は 20 × 30 × 40 mm の箱です。見本とライセンスは展開先に残してください。

Click 開く (Open) and choose samples/review-sample.glb for the cart or samples/review-sample.stl for the box. Drag to orbit and scroll to zoom. The interface is in Japanese.

ソースからビルドした版の画面は Apple Silicon の Mac で確認しています。Windows と Linux は実機の画面が未確認の試験版です。
The window of the source build has been tested on an Apple Silicon Mac. Windows and Linux builds are experimental, with no physical-machine UI verification.
