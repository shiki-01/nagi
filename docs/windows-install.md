# Nagi for Windows: インストールと更新

Nagi は、ローカルの LLM と辞書で変換する日本語入力です。この文書は Windows 版の導入手順です
（zip の中にも `README.md` として同梱しています）。

- 対象: Windows 10 22H2 以降 / Windows 11（x64）。ARM64 は未対応
- 状態: **実機での検証が済んでいません**（開発中のプレビュー。`docs/real-hardware-checklist.md` §8）。動作しない・不安定な場合があります
- 実機での検証が済むまで、リリースは prerelease として公開します。その間は方法 A（`irm | iex`）が使えないことがあります。方法 B で取得してください
- 入力内容・個人情報は外部へ送りません。ネットワークを使うのは、辞書・モデルの取得と更新の確認だけです（更新の確認は設定で無効にできます）

## インストール

### 方法 A（推奨）: PowerShell で 1 行

通常の PowerShell（管理者でなくてよい）で次を実行します。

```powershell
irm https://nagi.shiki-01.com/install.ps1 | iex
```

最新版の zip を取得し、SHA-256 を確かめてから、同梱のインストーラを実行します。途中で、ユーザーアカウント制御（UAC）の確認が出ます（`C:\Program Files\Nagi` への配置と、入力方式の登録に管理者権限が必要です）。
続けて、辞書とモデルを配布元から取得します（約 150MB。取得済みのものは飛ばします）。辞書だけで使う場合や後で取得する場合は、実行前に `$env:NAGI_SKIP_DATA = '1'` を設定します。

この方法でダウンロードしたファイルには「インターネットから取得」の印（Mark of the Web）が付かないため、SmartScreen の警告は出ません。

### 方法 B: zip を手動でダウンロードする

1. [GitHub Releases](https://github.com/shiki-01/nagi/releases) の `windows-vX.Y.Z` から `Nagi-windows-X.Y.Z.zip` をダウンロードします。
   - ブラウザが「一般的にダウンロードされていません」などと警告することがあります（署名していない配布物のため。下の「SmartScreen について」）。Edge ではダウンロード一覧の「…」→「保存」→「詳細表示」→「保持する」の順に進みます。
2. **展開する前に** zip を右クリック →「プロパティ」→ 全般タブ下部の「許可する」（ブロックの解除）にチェックして OK を押します。この手順を飛ばすと、展開したファイルすべてに警告の印が付きます。
3. zip を展開します（右クリック →「すべて展開」）。
4. 展開したフォルダで PowerShell を開き、実行します。

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
   ```

   `.ps1` を直接ダブルクリックしても実行されません（既定の実行ポリシーが Restricted のため）。必ず上のコマンドで実行してください。
5. 「Windows によって PC が保護されました」と表示された場合は、「詳細情報」→「実行」を押します。

インストーラのオプション: `-SkipData`（辞書とモデルを取得しない）、`-Model small|xsmall|none`（取得するモデル。既定は small）、`-InstallDir <パス>`（配置先。既定は `C:\Program Files\Nagi`）。

### インストール後の設定

1. 設定 > 時刻と言語 > 言語と地域 > 日本語 >「…」>「言語のオプション」>「キーボードを追加する」で **Nagi** を選びます。
2. Win+Space（または、タスクバーの入力方式の表示）で Nagi に切り替えます。開いているアプリでは、アプリを開き直すと反映されます。

管理者権限のアカウントと、実際に使うアカウントが別の場合（UAC で別のアカウントの資格情報を入力した場合）は、入力方式の登録が使うアカウントに反映されないことがあります。その場合は、使うアカウントで管理者として PowerShell を開き直し、同じ手順を実行してください。

## 配置されるもの

| 場所 | 内容 |
| --- | --- |
| `C:\Program Files\Nagi\` | `nagi_windows_tsf.dll`（入力方式）、`imserver.exe`（変換エンジン）、`uninstall.ps1`、ライセンス、`VERSION`、`SHA256SUMS` |
| `%APPDATA%\Nagi\data\` | 辞書（`dict\nagi-dict.bin`）とモデル（`models\*.gguf`）。配布元から取得したもの |
| `%APPDATA%\Nagi\` | 設定（`config.toml`）、学習データ、ユーザー辞書 |
| レジストリ | COM の登録（`HKCR\CLSID`）、TSF の入力方式、「アプリと機能」の項目（`HKLM\...\Uninstall\Nagi`） |

Program Files に置くのは、UWP などの保護されたアプリにも DLL を読み込ませるためです。

## 更新

方法 A のコマンドをもう一度実行します（上書きします）。使用中のファイルは名前を変えて退避されるため、そのまま置き換えられます。**サインアウトするか再起動すると**、すべてのアプリで新しい版にそろいます。

新しい版があるときは、起動時の更新確認で通知します（設定 `[update] auto_check`。無効にできます）。アプリ内での自動の置き換えは、Windows 版ではまだ実装していません。

## アンインストール

「設定 > アプリ > インストールされているアプリ」の「Nagi（日本語入力）」、または次のコマンドで行います。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "C:\Program Files\Nagi\uninstall.ps1"
```

- 入力方式の登録解除（`regsvr32 /u`）と、`C:\Program Files\Nagi` の削除をします。使用中で消せないファイルは、再起動時に削除されます。
- 設定・学習データ・ユーザー辞書・取得した辞書とモデル（`%APPDATA%\Nagi`）は**残します**。すべて消すには `-RemoveUserData` を付けます（確認で `yes` の入力が要ります。`-Force` で省略）。学習データだけを消したい場合は、`%APPDATA%\Nagi\learning.tsv` を削除してください。

## SmartScreen について

この配布物は Authenticode（コード署名）で署名していません。署名の証明書は有償で、現時点では用意していないためです。そのため、ブラウザなどでダウンロードした zip や実行ファイルには、Microsoft Defender SmartScreen が「一般的にダウンロードされていません」「Windows によって PC が保護されました」と警告することがあります。

- 方法 A は、警告の原因になる印が付かない取得方法です。配布元（`nagi.shiki-01.com`・GitHub）と HTTPS を信頼できる場合に使ってください。
- 方法 B で警告が出る場合は、上の手順（zip のブロック解除、または「詳細情報」→「実行」）で進められます。ファイルの正しさは、zip と同時に公開している `latest.json` の SHA-256 と照合できます。

  ```powershell
  (Get-FileHash .\Nagi-windows-X.Y.Z.zip -Algorithm SHA256).Hash
  ```
- ソースコードは公開しています。自分でビルドすることもできます（手順は `.github/workflows/windows-release.yml` が参照になります）。

将来の方針（署名の導入）は `docs/decisions.md` H-42 にあります。

## うまくいかないとき

- **入力方式に Nagi が出ない**: サインアウトして入れ直す。`regsvr32` の登録が失敗していないか、管理者の PowerShell で `regsvr32 "C:\Program Files\Nagi\nagi_windows_tsf.dll"` を実行して確かめる。
- **変換されず読みがそのまま出る**: 辞書が無い状態です。方法 A をもう一度実行するか、`"C:\Program Files\Nagi\imserver.exe" --install-data --data-dir "$env:APPDATA\Nagi\data" --model small` を実行して辞書とモデルを取得します。
- **`imserver.exe` が起動しない**: [Visual C++ 再頒布可能パッケージ（x64）](https://aka.ms/vs/17/release/vc_redist.x64.exe) を入れる。
- **エンジンが停止した通知が出る**: 自動で再起動します。繰り返す場合は `imserver.exe` を端末から直接実行して、エラーの内容を確かめてください。
