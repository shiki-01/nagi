# Windows フロント 実機検証手順書

Phase 5（Windows フロント）の実機検証の手順書です。確認項目の一覧は `docs/real-hardware-checklist.md` §8 にあり、この文書は「どの順に、どのコマンドで、何を見るか」を定めます。決定の経緯は `docs/decisions.md` H-39〜H-43 を参照してください。

- 本文中の「要確認」は、材料（ソース・ワークフロー・設計書）から確かめられず、推測を含む箇所です。検証の際に実際の挙動で埋めてください。
- 現状、`frontend-windows/` と `frontend-windows-tsf/` は Linux 上の型検査（`x86_64-pc-windows-gnu`）と単体テストを通っただけで、Windows 実機では一度も動かしていません。想定外の失敗を前提に進めてください。

## 1. 目的と前提

### 目的

- TSF の入力方式（`nagi_windows_tsf.dll`）と変換エンジン（`imserver.exe`）が、Windows 実機でインストール・登録・入力・更新・削除まで一通り動くことを確認する。
- 実機で確認が要る点（H-39〜H-43 の「未検証」の項目）を潰し、結果を `docs/real-hardware-checklist.md` §8 と `docs/decisions.md` に反映する。

### 対象 OS

- Windows 10 22H2 以降 / Windows 11（x64）。`install.ps1` は Build 19045 未満と x64 以外（ARM64 を含む）を拒否します。
- ARM64 Windows は検証対象外です（§5）。

### 必要なもの

| 項目 | 用途 | 備考 |
| --- | --- | --- |
| 管理者権限のアカウント | `regsvr32` による登録、`C:\Program Files\Nagi` への配置 | UAC が出る環境で検証する |
| Rust 1.89.0 + MSVC Build Tools | 段階 A（ローカルビルド） | `core/rust-toolchain.toml` で 1.89.0 固定 |
| cmake、LLVM（`LIBCLANG_PATH`） | `--features llama` でビルドする場合のみ | CI は `C:\Program Files\LLVM\bin` を使用 |
| `gh`（GitHub CLI） | 段階 B（ワークフローの実行と成果物の取得） | 任意。ブラウザの Actions 画面でも可 |
| Git | リポジトリの取得 | |
| 対象環境の VM またはスナップショット | 段階 C〜F | 後述 |

### 検証環境についての注意

- **VM かスナップショットで行うことを推奨します。** TSF の登録はレジストリに残り、`.dll` はホストプロセスに読み込まれ続けるため、巻き戻せる環境のほうが安全です。クリーンな状態（VC++ 再頒布可能パッケージなし）のスナップショットを 1 つ持っておくと、§3 段階 F の「VC++ なし」の確認にも使えます。
- 実機（物理 PC）でしか見られない項目があります（電源・バッテリー節約・サーマル推定、スリープ復帰、複数モニターと DPI）。VM では確認できない項目は、記録時に「VM では不可」と書いて、後で実機で確認してください。
- **実ユーザーの学習データを汚さないこと。** 普段使いの Windows アカウントでインストールすると、`%APPDATA%\Nagi`（`config.toml`・`learning.tsv`・ユーザー辞書・取得した辞書とモデル）に検証用の入力が混ざります。検証専用のアカウントまたは VM を使ってください。単体で `imserver` を動かす場合は必ず `--user-dir` を指定します（§3「デバッグの方法」）。
- 検証中の入力内容（パスワード等の実データ）をテキストに使わないでください。

## 2. 検証の進め方の全体像

段階 A → F の順に進めます。前段が失敗した場合は、原因を切り分けて直すまで後段に進みません。

| 段階 | 内容 | 次に進む基準 | 失敗したら |
| --- | --- | --- | --- |
| A | ビルド・単体テスト（ローカル） | `cargo test` と `cargo build --release` がすべて通り、DLL と `imserver.exe` ができる | コードの問題。後段へ進まず修正を依頼する |
| B | CI（`windows-release.yml`）での zip 作成 | ワークフローが成功し、zip と `latest.json` が artifact に残る | ビルド環境（MSVC・libclang・cmake・llama.cpp）の問題。`llama: false` で切り分けて再実行 |
| C | インストール（zip 手動 / `irm \| iex` / `install.ps1`） | `C:\Program Files\Nagi` への配置、`regsvr32` の登録、辞書・モデルの取得が成功する | 登録できなければ段階 D に進めない。§3 の切り分けに従う |
| D | 登録とホストアプリでの読み込み | 設定に Nagi が出て、メモ帳で切り替えられ、ホストが落ちない | DLL の読み込み・COM 登録の問題。段階 E に進まない |
| E | 入力動作 | 8.3〜8.5c の主要項目（入力・変換・確定・候補ウィンドウ）が動く | 不具合は項目ごとに記録し、致命的なもの（入力が壊れる・ホストが落ちる）が直るまで更新・削除の検証（F）に進まない |
| F | 更新・アンインストール | 上書き更新と削除が成功し、残骸がない | 次回の検証の前にスナップショットへ戻す |

致命的な失敗の目安は次のとおりです。

- ホストアプリがクラッシュする、フリーズする（段階 D 以降は即中止）。
- 入力した文字が失われる、意図せず確定される、キーが効かなくなる。
- 登録解除ができず、入力方式が残る。

## 3. 段階ごとの手順

### 段階 A: ビルド・単体テスト（8.1）

Windows ホストで実施します。コマンドはすべて `core\` で実行します。

```powershell
cd core
cargo test -p nagi-windows -p nagi-windows-tsf
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

DLL（cdylib、CI と同じく CRT 静的リンク）と `imserver.exe` のビルド:

```powershell
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --target x86_64-pc-windows-msvc -p nagi-windows-tsf
Remove-Item Env:RUSTFLAGS
# llama 有り（LIBCLANG_PATH と cmake が必要）
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
cargo build --release --target x86_64-pc-windows-msvc -p nagi-server --bin imserver --features llama
# llama 無し（辞書のみ）
cargo build --release --target x86_64-pc-windows-msvc -p nagi-server --bin imserver
```

成果物（`--target` 指定時）: `core\target\x86_64-pc-windows-msvc\release\nagi_windows_tsf.dll` と `imserver.exe`。

ローカルで zip まで作る場合（任意。署名なしで作成されます）:

```powershell
cargo xtask bundle-windows --target x86_64-pc-windows-msvc
```

出力は `core\target\windows-bundle\Nagi-windows-<version>.zip` です（`CARGO_TARGET_DIR` を設定している場合はその下）。`THIRD_PARTY_LICENSES_WINDOWS.md` などの元ファイルがリポジトリに必要です。

期待結果と確認点:

- [ ] `nagi-windows` の `process.rs`（`ProcessTransport`）のテストが Windows で通る（Linux では `cfg(unix)` のテストのみ実行していたため、Windows 側は初めて動く）。
- [ ] DLL ができる。`dumpbin /dependents nagi_windows_tsf.dll`（Developer Command Prompt）に `vcruntime140.dll` が出ない（CRT 静的リンクの確認。要確認: `dumpbin` が使える前提）。
- [ ] 子プロセス起動時にコンソールウィンドウが出ない（`CREATE_NO_WINDOW`。段階 D で目視）。

失敗時の切り分け:

- `error: linker ... not found` → MSVC Build Tools（C++ によるデスクトップ開発）が未導入。
- `llama` で `libclang` のエラー → `LIBCLANG_PATH` が `libclang.dll` のあるディレクトリを指しているか確認。cmake が `PATH` にあるか確認。
- テストだけ失敗 → どのテストか記録して報告する（`process.rs` のテストは Windows 側が未検証のため、テスト側の不備の可能性もある）。

### 段階 B: CI（`windows-release.yml`）（8.7 の最初の項目）

`windows-release.yml` は `workflow_dispatch` のみです（`ci.yml` も手動実行のみ）。実行する版は `main` の最新コミットで、リリースするバージョンは `core/Cargo.toml` の `[workspace.package] version` から自動で決まります（入力は不要）。変更はあらかじめ `main` に push してください。

入力（`windows-release.yml` から確認済み）: `llama`（既定 true）/ `publish`（既定 true）/ `prerelease`（既定 true）。

公開せずにビルドだけ確認する場合:

```powershell
gh workflow run windows-release.yml --ref main -f publish=false
gh run list --workflow windows-release.yml --limit 1
gh run watch <run-id>
gh run download <run-id> -n nagi-windows-X.Y.Z -D <保存先ディレクトリ>
```

artifact 名は `nagi-windows-X.Y.Z`、中身は `Nagi-windows-X.Y.Z.zip` と `latest.json` です。

確認点（Actions のログ）:

- [ ] 「バージョンの確認」「Secrets の確認」が通る。`NAGI_UPDATE_SIGNING_KEY` が未設定だと失敗する（必須の Secrets）。`WINDOWS_CODESIGN_PFX` 未設定は notice のみ。
- [ ] 「libclang の場所」（`llama` のとき）、「TSF の DLL をビルド（CRT 静的リンク）」、「imserver をビルド」が通る。Windows ランナーでの llama ビルドは Actions 上で一度も実行していません（H-42）。
- [ ] 「bundle」が通り、`Nagi-windows-X.Y.Z.zip` ができる。
- [ ] 「インストーラのスモークテスト」: regsvr32 の登録と解除を含む。`continue-on-error` のため失敗しても全体は成功扱い。**ログを必ず開いて結果を確認する**（失敗は警告のみで見落としやすい）。
- [ ] 「latest.json（ed25519 署名つき）」が通る。

失敗時の切り分け:

- llama のステップだけ失敗 → `-f llama=false` で再実行し、辞書のみのビルドが通るか確認する（これは暫定の逃げ道。llama 有りでの再現手順を記録）。
- バージョンは `core/Cargo.toml` から自動で決まる。意図した値か、ジョブ冒頭の notice（「リリースするバージョン」）で確認。
- 「リリース windows-vX.Y.Z は作成済みです」→ `publish=false` でも `gh release view` で作成済みを検査するため、未公開の version を使うか既存リリースを削除（削除は呼び出し元の判断）。

公開して検証する場合（実機で未検証の間は prerelease の既定のまま）:

```powershell
gh workflow run windows-release.yml --ref main
```

`prerelease=true` の間は `web.yml` が `latest.json` を取り込まないため、サイトの `irm | iex` は 404 になります（想定どおり）。

### 段階 C: インストール（8.7）

以降は VM またはスナップショットを取った Windows で実施します。

#### C-1 zip を手動で展開して `install.ps1`（方法 B）

1. GitHub Releases の `windows-vX.Y.Z` から `Nagi-windows-X.Y.Z.zip` を取得する（段階 B の artifact でも可。その場合 Mark of the Web が付かないため、SmartScreen の確認には使えない）。
2. ブラウザ（Edge と Chrome の両方）で取得し、**警告の出方を記録**する（後述の SmartScreen の項）。
3. 展開前に zip を右クリック →「プロパティ」→「許可する」にチェックして OK。展開する。
4. 展開したフォルダで PowerShell を開いて実行する。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

オプション（`install.ps1` の `param` から確認済み）: `-SkipData`、`-Model small|xsmall|none`（既定 small）、`-InstallDir <パス>`（既定 `C:\Program Files\Nagi`）。`-Elevated` は内部用で、手動では指定しません。

期待結果:

- [ ] 管理者でない PowerShell から実行すると UAC の確認が出る。承認後、昇格した子プロセスが配置と登録だけを行う。
- [ ] `SHA256SUMS` の検証が通る（配置前）。
- [ ] `C:\Program Files\Nagi` に次が置かれる: `nagi_windows_tsf.dll`、`imserver.exe`、`uninstall.ps1`、`README.md`、`LICENSE-MIT`、`LICENSE-APACHE`、`NOTICE`、`THIRD_PARTY_LICENSES.md`、`VERSION`、`SHA256SUMS`。
- [ ] `regsvr32 /s` が成功する（終了コード 0）。
- [ ] 「設定 > アプリ > インストールされているアプリ」に「Nagi（日本語入力）」が出る（`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Nagi`）。
- [ ] 辞書・モデルの取得（約 150MB）が通り、`%APPDATA%\Nagi\data\dict\nagi-dict.bin` と `%APPDATA%\Nagi\data\models\*.gguf` ができる。
- [ ] VC++ ランタイムが無い環境では、`vcruntime140.dll` / `msvcp140.dll` の警告が出る。

確認コマンド:

```powershell
Get-ChildItem "C:\Program Files\Nagi"
Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Nagi'
Get-ChildItem "$env:APPDATA\Nagi\data" -Recurse | Select-Object FullName, Length
```

標準ユーザー（UAC で別アカウントの資格情報を入力）の場合の確認:

- [ ] 入力方式が**実際に使うアカウント**の言語設定に出るか。出ない場合は、`docs/windows-install.md` のとおり使うアカウントで管理者として開き直して再実行すると出るか。（`RegisterProfile` が昇格側のアカウントに掛かる可能性は H-42 の未検証事項。HKCU の TSF 登録がどこに書かれるかは要確認。）

#### C-2 `irm | iex`（方法 A）

PowerShell 5.1（`powershell.exe`）と PowerShell 7（`pwsh.exe`）の両方で、通常（管理者でない）の PowerShell から実行します。

```powershell
irm https://nagi.shiki-01.com/install.ps1 | iex
```

prerelease の間は `latest.json` がサイトに載らず 404 になるため、次のどちらかで代替します（どちらも要確認）。

- `web.yml` を手動実行して、公開済み（`prerelease=false`）の版でサイトに載せる。
- リポジトリの `web/static/install.ps1` を `NAGI_MANIFEST_URL` 付きで直接実行する。`latest.json` のアセット URL は `https://github.com/shiki-01/nagi/releases/download/windows-vX.Y.Z/latest.json` の形になる想定（要確認。非公開リポジトリなら認証が要り、`irm` では取れない）。

```powershell
$env:NAGI_MANIFEST_URL = 'https://github.com/shiki-01/nagi/releases/download/windows-vX.Y.Z/latest.json'
Get-Content .\web\static\install.ps1 -Raw | Invoke-Expression
```

オプション: `$env:NAGI_SKIP_DATA = '1'` で辞書・モデルの取得を飛ばす。

期待結果:

- [ ] `latest.json` の取得、zip のダウンロード、SHA-256 の検証、展開、同梱の `install.ps1` の実行まで進む。
- [ ] **SmartScreen の警告が出ない**（`Invoke-WebRequest` で取得したファイルには Mark of the Web が付かない）。
- [ ] 一時フォルダ（`%TEMP%\nagi-install-*`）が終了後に消える。
- [ ] `latest.json` の `sha256` が不一致なら止まる（`web/static/install.ps1` の挙動。異常系は必要なら `latest.json` を改変して確認）。

#### C-3 SmartScreen / Defender の見え方（署名なし配布）

- [ ] Edge と Chrome でブラウザから zip を取得したときの警告の文言と、続行手順（`docs/windows-install.md` の方法 B 1 項）で実際に進められること。
- [ ] zip のブロック解除をしないで展開した場合、`.ps1` を実行すると警告が出るか。`powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1` で実行できるか。
- [ ] 「Windows によって PC が保護されました」が出た場合に「詳細情報」→「実行」で進める。
- [ ] DLL を `ALL APPLICATION PACKAGES` が読めること（UWP の確認は段階 D）。

結果は画面のスクリーンショットまたは文言をそのまま記録してください（`docs/windows-install.md` の説明と食い違う場合は、同文書の修正が必要なので報告します）。

#### C の切り分け

- `regsvr32` が失敗 → 管理者の PowerShell で `regsvr32 "C:\Program Files\Nagi\nagi_windows_tsf.dll"` を実行し、ダイアログのエラーを記録する（`/s` なしならメッセージが出る）。
- 配置で「既存のファイルを置き換えられません」 → 使用中のファイルがある。サインアウトして再実行する。
- 辞書・モデルの取得失敗 → `& "C:\Program Files\Nagi\imserver.exe" --install-data --data-dir "$env:APPDATA\Nagi\data" --model small` を直接実行し、標準エラーのメッセージを記録する。

### 段階 D: 登録とホストでの読み込み（8.2）

#### D-1 登録

1. 設定 > 時刻と言語 > 言語と地域 > 日本語 >「…」>「言語のオプション」>「キーボードを追加する」で **Nagi** を選ぶ。
2. Win+Space で Nagi に切り替える。

- [ ] 入力方式の一覧に Nagi が出る。アイコン（藍色の角丸に白い「N」）が設定の「言語と地域」とタスクトレイに出る。
- [ ] `C:\Program Files\Nagi\nagi.ico` が作られている（`DllRegisterServer` が DLL と同じディレクトリに書き出す）。管理者権限で実行しているため書けるはずだが、書けないディレクトリ（`-InstallDir` に別の場所を指定）での挙動も任意で確認する。
- [ ] COM の登録が残っている: `reg query "HKCR\CLSID\{B0D6897E-C414-4BB1-B621-46EEE659E0CD}" /s`（CLSID は `frontend-windows-tsf/src/guids.rs`）。

#### D-2 ホストでの読み込み

メモ帳、ブラウザ（Edge / Chrome）、VS Code、Office（Word 等）、UWP / 保護されたアプリ（設定アプリ、Microsoft Store アプリ、Edge の一部）の各々で、Nagi に切り替えて 1 文字入力します。

- [ ] DLL が読み込まれる: `tasklist /m nagi_windows_tsf.dll`
- [ ] アプリを閉じる／入力方式を切り替えてもホストが落ちない（`DllCanUnloadNow` のアンロード）。
- [ ] DLL と同じディレクトリの `imserver.exe` が起動する: `Get-Process imserver`（ホストプロセスごとに 1 つ起動する設計。H-39）。コンソールウィンドウが出ない。
- [ ] UWP / 保護されたアプリ（設定アプリ・Store アプリ・Edge の一部）でも DLL が読み込まれる（`C:\Program Files\Nagi` の ACL に `ALL APPLICATION PACKAGES` の読み取り権限が付く前提。H-42 の見込み）。読み込まれない場合は `icacls "C:\Program Files\Nagi"` の出力を記録する。
- [ ] Nagi を選んでいないアプリ、または Activate されないプロセスで `imserver.exe` が起動しない（`TextService` は Activate までエンジンに触れない設計。H-41）。

#### D の切り分け

- 一覧に出ない → サインアウトして再確認。`regsvr32 /u` → `regsvr32` で登録し直す。
- 一覧には出るが入力できない → `tasklist /m nagi_windows_tsf.dll` に出るか。出なければ COM 登録（`InprocServer32` のパス）を確認。出るのに無反応なら、`imserver.exe` が起動しているかを確認（`Get-Process imserver`）。
- 起動しない `imserver.exe` → 直接実行してエラーを見る。VC++ 再頒布可能パッケージ（x64）の有無を確認。

### 段階 E: 入力動作（8.3〜8.5c）

辞書・モデルを取得した状態で、メモ帳を主に使い、各項目をその他のアプリでも確認します。「ホスト」欄には確認したアプリ名を記録します。

#### E-1 ローマ字入力・変換・確定（8.3）

- [ ] ローマ字入力でひらがなが composition（下線付き）として表示される。Enter で確定、Esc で取り消せる。
- [ ] 「きょうはいいてんきですね」等で Space → 変換結果が出る。←/→ で文節移動、Enter で確定できる。
- [ ] 辞書だけの場合と、LLM（モデル取得済み）の場合の差（`docs/progress.md` の確認方法に準拠。文脈に沿った 1 件目になるか）。
- [ ] 確定後にカーソルが文末にある（`ShiftEnd`）。

#### E-2 候補ウィンドウ（8.5・8.5b）

- [ ] Space 変換で候補ウィンドウが表示され、**フォーカスを奪わない**（入力中アプリのキャレットが消えない、タイトルバーが非アクティブにならない）。
- [ ] キャレット（composition）の直下に出る。メモ帳・ブラウザ・VS Code・Office・ターミナル・UWP で位置が合う。取れないアプリではマウス位置に出る。
- [ ] 画面下端（タスクバー付近）では上に反転する。複数モニターで作業領域からはみ出さない。
- [ ] DPI 100% / 150% / 200%、DPI の違う複数モニター（Per-Monitor 対応のホストと非対応のホストの両方）で大きさ・位置が合う。
- [ ] 選択行のハイライト、番号、出所ラベル（学習・AI・辞書・もしかして等）、ページ「2/5」が崩れない。候補が `page_size` を超えるとページが替わる。数字キーで選べる。
- [ ] 文字幅・行の高さ・余白が窮屈でない。

#### E-3 表示属性（8.5・8.5b）

- [ ] 通常 = 細い実線、注目文節・選択 = 太い実線、確定済み・保留ローマ字 = 点線、スタイル変換 = 破線がホストに反映される。確定後に下線が残らない。
- [ ] 属性を無視するアプリでも入力が壊れない。`IEnumTfDisplayAttributeInfo` を列挙するアプリが固まらない。
- [ ] 未変換を点線にするなど MS-IME 風にする要否を、見た目から判断して記録する（変更は `attr_for` の 1 か所）。

#### E-4 再変換（8.5b・8.5c）

- [ ] 確定済みテキストを選択して変換キー / かなキーを押すと composition になる。
- [ ] Office など `ITfFnReconversion` を呼ぶアプリで、再変換の候補一覧が出て、選ぶと範囲が置き換わりキャレットが末尾に来る。複数文節の範囲で、残りの文節が失われない。
- [ ] 選択が空・200 文字超のとき何もしない。入力中は再変換しない。
- [ ] 初回の逆引き索引の構築で待つ場合、通知が出て取り消せる（2 秒の待ち）。
- [ ] 呼んだ後にコアが変換中のまま残らない（直後の入力が通常どおり）。アプリが文書ロックを持つ最中に呼んで固まらない。

#### E-5 言語バー・モード（8.4・8.5b・8.5c）

- [ ] タスクトレイの表示が「あ / A」で切り替わる。出ない場合は下記「デバッグの方法 > ログ」の tsf.log とチェックリスト §8.5d を見る。
- [ ] 英数キー・半角/全角キー・トレイのクリックの往復でモードが反転し続けない。入力中の文字列が意図せず確定されない。
- [ ] 英数 = IME 閉のとき、アプリ側が IME オフ扱いで困らない。Activate 直後・エンジン再起動後に OS とコアのモードが揃う。
- [ ] 従来の言語バー（設定の「言語バー」を有効にした環境、またはデスクトップ言語バー）に「あ / A」のボタンが出る。ホスト切り替えや Deactivate でボタンが残らない。トレイの「あ / A」と二重に出ないか。
- [ ] 左クリックでひらがな ⇄ 英数、右クリックでメニュー（ひらがな / 英数・状態の行・「設定ファイルを開く…」）が出る。メニューで選ぶとモードが変わる。「設定ファイルを開く…」で `config.toml` がメモ帳で開く。
- [ ] 長時間・多数回のモード切り替えで GDI オブジェクトが増え続けない（タスクマネージャーの「GDI オブジェクト」列）。DPI 150/200% でアイコンが滲まない。

#### E-6 キー割り当て（8.4）

JIS 配列と US 配列の両方で確認します（`Win32` の仮想キー）。

- [ ] 半角/全角（`VK_KANJI` / `VK_OEM_AUTO` / `VK_OEM_ENLW`）でモードが切り替わる。
- [ ] 変換（`VK_CONVERT`）・無変換（`VK_NONCONVERT`）・カタカナひらがな（`VK_KANA`）の動作。
- [ ] `VK_IME_ON` → ひらがな、`VK_IME_OFF` → 英数が、OS の IME オン/オフと食い違わない。
- [ ] `VK_OEM_BACKTAB` が実際にどのキーに当たるか（HIRAGANA_KATAKANA に割り当てた判断が妥当か）を記録する。
- [ ] AltGr 配列で `@` などの記号が失われない（Ctrl+Alt 併用時に text を残す判断）。
- [ ] 修飾キー単独の押下・離上（`flags_changed`）。押下を食ったキーの離上を食わない。
- [ ] `OnTestKeyDown` だけが呼ばれ `OnKeyDown` が来ない場面の発生頻度（エンジン状態が先に進む問題）。発生するアプリを記録する。

#### E-7 ライブ変換・非同期通知の即時反映（8.5b）

- [ ] ライブ変換・LLM 結果の差し替えが、**次のキーを待たずに**画面に出る（隠しウィンドウ + `PostMessage`）。入力後 数百 ms で変わる。
- [ ] 差し替え直後のキー入力と順序が崩れない（`TF_ES_ASYNCDONTCARE` の編集セッションと同期セッションの順序）。
- [ ] 通知が別ウィンドウ（別スレッド）の入力に混ざらない。複数のホストを開いて同時に入力して確認する。

#### E-8 エンジンのクラッシュ復帰・スリープ復帰（8.3・8.5b）

エンジンの強制終了:

```powershell
Stop-Process -Name imserver -Force
```

- [ ] composition 中に強制終了しても、エンジン未接続中の素通し（composition 中だけ食う）で入力が壊れない。
- [ ] 自動で再起動し（指数バックオフ）、`restore_session` で preedit が復元される。通知トースト（エンジン停止・復元失敗）が右下に約 4 秒出てフォーカスを奪わない。同文言は 30 秒間引かれる。

スリープ復帰:

- [ ] composition 中にスリープ → 復帰すると、エンジンが再起動して preedit が復元される。二重に再起動しない（`Get-Process imserver` の数と PID を確認）。
- [ ] メッセージ専用ウィンドウの `WM_POWERBROADCAST` が届くか。届かない場合もタイマーの検出（30 秒周期）で復帰するか。どちらの経路で復帰したかを記録する。要確認: 判別方法（現状ログ出力がないため、復帰までの所要時間で推定する）。

#### E-9 入力のタイムアウト 250ms の妥当性（8.3）

キー処理の要求タイムアウト 250ms は実測なしの暫定値です（H-39）。

- [ ] 長文・連続入力・重い変換（LLM 有り、`balanced` / `quality`）でキーを取りこぼさない。
- [ ] 取りこぼしが出る場合、状況（入力速度・文の長さ・プロファイル・CPU 負荷）を記録し、タイムアウトの引き上げ案を添える。
- [ ] 計測が必要な場合は、`nagi-eval`（`cargo run --release -p nagi-server --features llama --bin nagi-eval -- --data-dir ../data`）のレイテンシ結果を参考にする（キー入力の p95 そのものではない点に注意）。

#### E-10 環境・文脈（8.5・8.5b）

- [ ] AC/電池の切替、バッテリー節約の切替が約 30 秒以内（電源通知が届けば即時）に `get_status` のプロファイルへ反映される。電池のないデスクトップ PC で電池駆動と誤判定されない。
- [ ] メモリを圧迫したときだけ light/minimal に落ちる（閾値は実測なし）。通常時に誤って軽いプロファイルに落ちない。サーマル推定がアイドル時や省電力プランで誤判定しない。
- [ ] 入力開始時にキャレット直前のテキストが送られ、次語候補・文脈が効く。矢印・ショートカット・修飾キーでは文書を読まない。
- [ ] パスワード欄（`InputScope` が `IS_PASSWORD`）で周辺テキストを読まず `secure_input: true` を送り、学習・ログが止まる。アプリ ID に `chrome.exe` 等が入る。
- [ ] 通知の文言（エンジン停止・minimal 移行・復元失敗・`update_available`）とモード表示（あ / A）が妥当。

プロファイルの確認は `get_status` のレスポンスまたは言語バーのメニュー（状態の行）で行います（`get_status` を直接呼ぶ方法は下記「デバッグの方法」）。

### デバッグの方法

#### ログ

- `imserver` のログは **stderr のみ**で、入力内容は出力しません（`core/server/src/main.rs`）。プロファイルの判定理由などが起動時に出ます。
- TSF の DLL から起動された `imserver` の stderr は親（ホストアプリ）から継承する設計（`frontend-windows/src/process.rs`）です。GUI アプリでは標準エラーが見えないため、**ホスト経由ではログを取れない可能性が高い**（要確認）。DLL 側のログは下記の tsf.log に出ます（`OutputDebugString` は使っていません）。
- そのため不具合の再現は、可能なものから順に次のいずれかで行います。
  1. 下記「imserver を単体で動かす」で、エンジン単体の問題か切り分ける。
  2. エンジンが起動しているか（`Get-Process imserver`）、DLL が読み込まれているか（`tasklist /m nagi_windows_tsf.dll`）を見る。
  3. Sysinternals の Process Monitor（`imserver.exe` / `nagi_windows_tsf.dll` のレジストリ・ファイルアクセス）、Process Explorer（DLL の読み込み先）、DebugView（`OutputDebugString` があれば。無い場合は何も出ない）を使う。
- **tsf.log（フロントの診断ログ。H-54）**: `%LOCALAPPDATA%\Nagi\logs\tsf.log`（256KB を超えると `tsf.log.1` へ 1 世代回る）。ホストプロセスごとに `pid=` 付きで追記される。入力内容（キー・テキスト・preedit）は書かない。記録するのは、DLL の Activate / Deactivate、エンジンの場所（`exe=… exists=…`）、起動の成否と失敗理由（`launch failed: spawn failed` / `initialize timeout` / `initialize failed` / `start failed: connection closed (engine exited)`）、クラッシュと再起動、minimal への切り替え、言語バーの登録結果（HRESULT）。書けない環境では黙って記録しない。
- **トレイのアイコンが出ない場合**: まず tsf.log を見る。`[tsf] Activate` が無い → DLL が読み込まれていない。`[langbar] AddItem failed hr=…` → その HRESULT を報告。`AddItem ok` なのに出ない → チェックリスト §8.5d の 3〜4（TIPCAP の再登録・サインアウト・入力インジケーターの設定）。エンジンの起動失敗とは独立（エンジンが無くてもアイコンは出る想定）。
- 「ログを残す機能が足りない」場合は検証結果として報告してください。

#### imserver を単体で動かして JSON-RPC を確認する

**必ず `--user-dir` を検証用のディレクトリに向けます。** 指定しないと `%APPDATA%\Nagi`（実際に使っている設定・学習・ユーザー辞書）を読み書きし、学習データを汚します。

```powershell
$user = Join-Path $env:TEMP 'nagi-verify-user'
New-Item -ItemType Directory -Force $user | Out-Null
& "C:\Program Files\Nagi\imserver.exe" --data-dir "$env:APPDATA\Nagi\data" --user-dir $user [--no-llm]
```

起動すると stderr にプロファイルが出ます。stdin に 1 行 1 JSON（JSON-RPC 2.0）を入力します。入力例（`frontend-ios-spike/README.md` の例にならったもの。Windows クライアントの `initialize` は `client_name: "nagi-windows"`、`os: "windows"` を送る実装。そのほかのパラメータは要確認）:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"nagi-verify","client_version":"0","os":"windows"}}
{"jsonrpc":"2.0","id":2,"method":"process_key","params":{"session_id":"verify-1","keysym":97,"keycode":0,"text":"a","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"char"}}
```

標準入力を閉じるとサーバーは終了します（要確認: EOF 後に未処理の非同期通知が出力されるか）。非対話で試す場合は、リクエストを 1 行 1 JSON でファイルに書いて流します。

```powershell
Get-Content .\requests.jsonl | & "C:\Program Files\Nagi\imserver.exe" --data-dir "$env:APPDATA\Nagi\data" --user-dir $user --no-llm
```

メソッド一覧（`get_status` を含む）は設計書 §3.2 を参照してください。この手順書では、`get_status` のパラメータを確認できていません（要確認）。

### 段階 F: 更新・アンインストール（8.7）

#### F-1 更新（上書きインストール）

入力方式の使用中（メモ帳を開いて Nagi に切り替えた状態）に、同じ版または新しい版で `install.ps1` を再実行します。

- [ ] 使用中の `nagi_windows_tsf.dll` / `imserver.exe` が `.old` に退避されて、新しいファイルが置かれる（`Get-ChildItem "C:\Program Files\Nagi" -Filter *.old`）。
- [ ] 動作中の imserver（旧版）は止められない。旧版のエンジンと新版の DLL が混在して問題を起こさない（アプリを開き直すまでの間）。
- [ ] サインアウトまたは再起動後に、すべてのアプリで新しい版にそろう（`VERSION` と、`Get-Process imserver | Select Path`）。
- [ ] 次回の `install.ps1`（または `uninstall.ps1`）で、使用中でなくなった `.old` が片付く。
- [ ] 「アプリと機能」の `DisplayVersion` が新しい版になる。
- [ ] 更新の通知: 旧版の imserver が `/updates/windows/latest.json` を取得し、新版があれば右下にトースト（「PowerShell でインストールコマンドを…」）が出る。`update_check.json` で 24 時間間引かれる（要確認: `update_check.json` の場所。ユーザーディレクトリ配下と推測）。公開リリース（`prerelease=false`）と `web.yml` の実行が前提。
- [ ] 更新の確認を無効にした設定（`[update] auto_check`）で通知が出ない。

#### F-2 アンインストール

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "C:\Program Files\Nagi\uninstall.ps1"
```

- [ ] 管理者でなければ UAC が出る。`regsvr32 /u` で登録が消え、設定の入力方式の一覧から Nagi が消える（反映にサインアウトや再起動が要ることがある）。
- [ ] `C:\Program Files\Nagi` が消える。使用中で消せなかったファイルは `.old` に退避され、再起動後に消える（`MoveFileEx(DELAY_UNTIL_REBOOT)`）。再起動後にディレクトリごと消えているか確認する。
- [ ] 「アプリと機能」の項目（`HKLM\...\Uninstall\Nagi`）が消える。
- [ ] COM の登録が消えている: `reg query "HKCR\CLSID\{B0D6897E-C414-4BB1-B621-46EEE659E0CD}"` が「見つかりません」になる。`nagi.ico` も消える。レジストリに残骸がない（`HKCU\Software\Microsoft\CTF\TIP` 配下の登録の確認は要確認）。
- [ ] 既定では `%APPDATA%\Nagi`（設定・学習・ユーザー辞書・辞書とモデル）が**残る**。

`-RemoveUserData` の確認（別のスナップショットで）:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "C:\Program Files\Nagi\uninstall.ps1" -RemoveUserData
powershell -NoProfile -ExecutionPolicy Bypass -File "C:\Program Files\Nagi\uninstall.ps1" -RemoveUserData -Force
```

- [ ] 前者は確認で `yes` を入力しないと中止する（終了コード 1、何も変更しない）。`yes` で `%APPDATA%\Nagi` が削除される。
- [ ] `-Force` は確認を省略する。
- [ ] 動作中の `imserver` を止めてから削除する（学習データを書き戻されない）。

#### F-3 クリーン環境と ARM64（任意）

- [ ] VC++ 再頒布可能パッケージがない環境（クリーンな Windows）で、DLL（静的 CRT）は読み込まれ、`imserver.exe`（動的 CRT）は起動しないか、警告が出るか。`install.ps1` の警告が出ることも確認する。
- [ ] ARM64 Windows（エミュレーション）で x64 の DLL がホストに読み込まれるか。`install.ps1` は ARM64 を拒否するため、DLL を手で `regsvr32` した場合の確認になる（要確認。H-42 では未対応の扱い）。
- [ ] Authenticode 署名（Secrets を登録した場合のみ）: `signtool verify /pa /q nagi_windows_tsf.dll`（`imserver.exe` も）が通り、署名した DLL / exe が読み込まれる。

## 4. 結果の記録方法

### チェックリスト §8 の項目との対応

| 本書の段階 | `real-hardware-checklist.md` の節 |
| --- | --- |
| A | 8.1 |
| B | 8.7（`windows-release.yml` の項目） |
| C | 8.7（zip、`install.ps1`、`irm \| iex`、SmartScreen、標準ユーザー） |
| D | 8.2、8.7（UWP / Program Files） |
| E-1 | 8.3 |
| E-2 | 8.5、8.5b（候補ウィンドウ、キャレット矩形） |
| E-3 | 8.5、8.5b（表示属性） |
| E-4 | 8.5b（再変換）、8.5c（`GetReconversion`） |
| E-5 | 8.5b（入力モード）、8.5c（言語バーのボタン） |
| E-6 | 8.4 |
| E-7 | 8.3（ライブ変換の即時反映）、8.5b（非同期通知） |
| E-8 | 8.3（再起動）、8.5b（スリープ復帰、通知トースト） |
| E-9 | 8.3（250ms のタイムアウト） |
| E-10 | 8.5、8.5b（環境・文脈） |
| F-1 | 8.7（更新、更新の通知） |
| F-2 | 8.7（`uninstall.ps1`） |
| F-3 | 8.7（VC++、ARM64、Authenticode）、8.5c（`THIRD_PARTY_LICENSES_WINDOWS.md`） |
| （別枠） | 8.5c の `THIRD_PARTY_LICENSES_WINDOWS.md`（`cargo tree --target x86_64-pc-windows-msvc` を Windows 上で実行して比較）、8.6（エンジン共有の実測項目。H-43 §4） |

### 記録テンプレート

項目ごとに次の形式で記録します（検証ログとして `docs/decisions.md` へ書く際もこの形に揃えます）。

```text
項目: （例）E-2 候補ウィンドウ / DPI 150% のモニター
日付: YYYY-MM-DD
環境: Windows 11 23H2（Build 22631.xxxx）/ 物理 PC or VM（種類）/ DPI / 配列（JIS・US）
バージョン: Nagi vX.Y.Z（C:\Program Files\Nagi\VERSION）/ llama 有無 / モデル（small・xsmall）
ホスト: メモ帳 / Edge / ...
結果: OK / NG / 一部NG / 未確認（VM では不可 など）
メモ: 再現手順、期待との差、スクリーンショットの場所、エラーメッセージ
```

OS ビルドの取得: `[Environment]::OSVersion.Version`、または `winver`。

### 結果の反映先

1. `docs/real-hardware-checklist.md` §8: 確認した項目のチェックボックスを `[x]` にし、日付と結果を追記する。失敗した項目は `[ ]` のまま原因と日付を書く。
2. `docs/decisions.md`: 設計判断を変える結果（250ms の変更、`VK_OEM_BACKTAB` の割り当て、表示属性の見た目、エンジン共有（H-43 §4）の要否、署名の方針など）は、新しい H 番号の節として追記する。H-39〜H-43 の「未確認」を覆す場合は、どの節のどの点かを明示する。
3. `docs/progress.md` の Windows 節は、実機での検証の状況が変わったときに更新する。
4. 実機で確認できて `prerelease` をやめる判断（`-f prerelease=false`）は、§8.7 の結果がそろってから、呼び出し元（人）が行う。

`CLAUDE.md` と `docs/` の編集は、人または呼び出し元の判断で行います。

## 5. 既知の未実装と重点リスク

### 検証対象外（未実装）

- 複数ホストでのエンジン共有（名前付きパイプ + 常駐ブローカー）。設計メモのみ（H-43 §4）。ただし、ホストごとに `imserver` が起動するメモリ・起動時間・`learning.tsv` の取りこぼしの実測は検証対象（§8.6）。
- アプリ内の自動更新（ダウンロード・差し替え・再起動）。通知のみで、更新は `install` コマンドの再実行。
- ARM64 / 32bit ホスト対応。`install.ps1` は ARM64 を拒否する。
- 辞書・モデルのアプリ内ダウンロード導線。
- Authenticode 署名の実施（Secrets 未登録）、winget / MSI。
- 設定 GUI（Phase 4）。設定は `config.toml` をメモ帳で開く入口のみ。
- 再変換候補の選択結果の学習。
- 候補ウィンドウのマウス操作（v1 の非目標）。

### 重点的に見るリスク（H-39〜H-43 の「実機で確認が要る」点）

次は失敗時の影響が大きく、先に確認することを勧めます。

1. **候補ウィンドウがフォーカスを奪わないこと・位置と DPI**（H-41）。`WS_EX_NOACTIVATE`、`GetTextExt` が取れるアプリの範囲。
2. **非同期の編集セッションと直後のキー入力の順序**（H-41 (7)）。ライブ変換の差し替えで入力が乱れないか。
3. **メッセージ専用ウィンドウで `WM_POWERBROADCAST` を受け取れるか**（H-41 (2)）。受け取れなくても 30 秒タイマーで追従するか。
4. **コンパートメントの書き込み順と英数 = IME 閉の扱い**（H-41 (4)(6)）。モード表示が反転し続けない・アプリ側が困らない。
5. **`ITfFnReconversion`（`Reconvert` / `GetReconversion`）を実際に呼ぶアプリ**（Office 等）での挙動（H-41 (5)、H-43）。文書ロックの外で呼ぶか、`SetResult` の呼び方。
6. **言語バーのボタン**（H-43）: `ITfLangBarItemMgr` の取得、`BTN_BUTTON` の右クリックで `InitMenu` が呼ばれるか、`GetIcon` の `HICON` の所有権（GDI リーク）、トレイとの二重表示。
7. **Program Files の DLL が UWP / 保護されたアプリで読み込まれること**（H-42 (2)）。
8. **標準ユーザー + 別アカウントで UAC の場合の入力方式の登録先**（H-42）。
9. **`regsvr32` の登録と UAC の流れ、`-Elevated` 子プロセスの終了コード、更新時の `.old` 退避、アンインストール時の再起動後削除、`irm | iex`（PowerShell 5.1 / 7）**（H-42 (1)(3)(4)(5)）。
10. **GitHub Actions 上の Windows ランナーでの llama ビルド、`signtool` の場所、スモークテスト**（H-42 (6)）。
11. **実測なしの閾値**: キー処理 250ms のタイムアウト（H-39）、メモリ逼迫（利用可能 5%/10%・300MB/800MB）、サーマル推定（周波数 / 上限の比 85%/60%/40%）。
12. **`VK_OEM_BACKTAB` の実キー割り当て**、`IS_PASSWORD`（値 31）のパスワード欄判定（H-39、H-41）。
13. **ホストごとの `imserver` 起動のメモリ・起動時間**（H-39、H-43 §4）。ホストを 1 / 5 / 10 個開いたときの合計メモリ（ワーキングセット・コミットサイズ）、`initialize` の所要、`learning.tsv` の取りこぼし、`reconvert` 後のピーク（約 270MB の逆引き索引、G-29）を記録する。

## 付録: 「要確認」の一覧

- `dumpbin` による静的 CRT の確認方法（段階 A）
- TSF の登録が HKCU のどこに書かれるか、標準ユーザー・別アカウント UAC での登録先（段階 C-1、F-2）
- prerelease の `latest.json` アセットの URL 形式と、`NAGI_MANIFEST_URL` を使った `web/static/install.ps1` の実行方法（段階 C-2）
- ホスト経由で起動した `imserver` の stderr が見えるか、DLL 側のログ出力の有無（デバッグの方法）
- `initialize` / `process_key` の手動リクエストの詳細なパラメータ、`get_status` のパラメータ、EOF 後の挙動（デバッグの方法）
- スリープ復帰の経路（`WM_POWERBROADCAST` かタイマーか）の判別方法（E-8）
- `update_check.json` の場所（F-1）
- ARM64 で x64 の DLL を手で登録した場合の挙動（F-3）
