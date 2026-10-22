<#
.SYNOPSIS
  Nagi（日本語入力）をインストールする。

.DESCRIPTION
  展開した zip（Nagi-windows-X.Y.Z）の中から実行する。
    powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1

  1. 同梱ファイルを SHA256SUMS で検証する
  2. 管理者権限で %ProgramFiles%\Nagi に配置し、regsvr32 で入力方式（TSF）を登録する
     （管理者でなければ UAC の確認を出して、この手順だけを昇格して実行する）
  3. 辞書とモデルを配布元から取得する（現在のユーザーの %APPDATA%\Nagi\data。昇格しないで実行する）

  辞書とモデルは同梱・再配布しない。この PC が配布元から直接取得し、SHA-256 を検証して辞書を組み立てる。
  入力内容や個人情報は送らない。更新は同じ手順の再実行（上書き）で行う。

.PARAMETER InstallDir
  配置先。既定は %ProgramFiles%\Nagi。UWP / 保護されたアプリにも DLL を読ませるため、
  利用者のプロファイルの外（Program Files）に置く。

.PARAMETER Model
  取得する変換モデル。small（約 74MB、既定）/ xsmall / none（辞書だけ）。

.PARAMETER SkipData
  辞書とモデルを取得しない。

.PARAMETER Elevated
  内部用。昇格した子プロセスが配置と登録だけを行う。
#>
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:ProgramFiles 'Nagi'),
    [ValidateSet('small', 'xsmall', 'none')]
    [string]$Model = 'small',
    [switch]$SkipData,
    [switch]$Elevated
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$DllName = 'nagi_windows_tsf.dll'
$ExeName = 'imserver.exe'
# 配置するファイル（SHA256SUMS に載っているもの + SHA256SUMS 自身）
$PayloadFiles = @(
    'nagi_windows_tsf.dll', 'imserver.exe', 'uninstall.ps1', 'README.md',
    'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD_PARTY_LICENSES.md',
    'VERSION', 'SHA256SUMS'
)
$UninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Nagi'

function Say([string]$Message) { Write-Host "nagi: $Message" }

function Test-Administrator {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# 64bit の regsvr32。32bit の PowerShell から呼ぶと System32 が SysWOW64 に読み替えられるため Sysnative を使う
function Get-Regsvr32 {
    if ([Environment]::Is64BitProcess) {
        return (Join-Path $env:SystemRoot 'System32\regsvr32.exe')
    }
    return (Join-Path $env:SystemRoot 'Sysnative\regsvr32.exe')
}

function Test-Platform {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'Windows 専用です'
    }
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw '64bit 版の Windows が必要です'
    }
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    if ($arch -ne 'AMD64') {
        throw "この版は x64 の Windows 用です（この PC: $arch）。ARM64 には未対応です"
    }
    # Windows 10 22H2 = 19045、Windows 11 = 22000 以降
    if ([Environment]::OSVersion.Version.Build -lt 19045) {
        throw 'Windows 10 22H2 以降が必要です'
    }
}

# SHA256SUMS と実ファイルを突き合わせる（破損・取り違えの検出。改ざん対策ではない）
function Test-Payload([string]$SourceDir) {
    $sumsPath = Join-Path $SourceDir 'SHA256SUMS'
    if (-not (Test-Path -LiteralPath $sumsPath)) {
        throw "SHA256SUMS が見つかりません: $sumsPath（zip を展開し直してください）"
    }
    $listed = @{}
    foreach ($line in Get-Content -LiteralPath $sumsPath -Encoding UTF8) {
        if ($line -match '^([0-9a-f]{64})  (.+)$') {
            $listed[$Matches[2]] = $Matches[1]
        }
    }
    foreach ($name in @($DllName, $ExeName)) {
        if (-not $listed.ContainsKey($name)) { throw "SHA256SUMS に $name がありません" }
    }
    foreach ($name in $listed.Keys) {
        $path = Join-Path $SourceDir $name
        if (-not (Test-Path -LiteralPath $path)) { throw "ファイルがありません: $name" }
        $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $listed[$name]) { throw "SHA-256 が一致しません: $name（zip を取得し直してください）" }
    }
}

# 使用中（読み込み中の DLL・実行中の exe）でも置き換えられるよう、古いファイルは名前を変えて退避する
function Copy-Replacing([string]$From, [string]$To) {
    if (Test-Path -LiteralPath $To) {
        $old = "$To.old"
        Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
        try {
            Move-Item -LiteralPath $To -Destination $old -Force
        } catch {
            throw "既存のファイルを置き換えられません: $To（$($_.Exception.Message)）"
        }
    }
    Copy-Item -LiteralPath $From -Destination $To -Force
    Unblock-File -LiteralPath $To -ErrorAction SilentlyContinue
}

# 管理者権限が要る手順: 配置・登録・「アプリと機能」への登録
function Install-Privileged([string]$SourceDir) {
    Test-Payload $SourceDir

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($name in $PayloadFiles) {
        Copy-Replacing (Join-Path $SourceDir $name) (Join-Path $InstallDir $name)
    }
    # 前回までの退避ファイル（使用中でなくなったもの）を片付ける
    Get-ChildItem -LiteralPath $InstallDir -Filter '*.old' -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue

    $dll = Join-Path $InstallDir $DllName
    $process = Start-Process -FilePath (Get-Regsvr32) -ArgumentList @('/s', ('"{0}"' -f $dll)) -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "regsvr32 による登録に失敗しました（終了コード: $($process.ExitCode)）"
    }

    $version = (Get-Content -LiteralPath (Join-Path $InstallDir 'VERSION') -TotalCount 1).Trim()
    $uninstallScript = Join-Path $InstallDir 'uninstall.ps1'
    New-Item -Path $UninstallKey -Force | Out-Null
    $values = @{
        DisplayName     = 'Nagi（日本語入力）'
        DisplayVersion  = $version
        Publisher       = 'shiki-01'
        InstallLocation = $InstallDir
        URLInfoAbout    = 'https://nagi.shiki-01.com'
        UninstallString = ('powershell.exe -NoProfile -ExecutionPolicy Bypass -File "{0}"' -f $uninstallScript)
    }
    foreach ($key in $values.Keys) {
        New-ItemProperty -Path $UninstallKey -Name $key -Value $values[$key] -PropertyType String -Force | Out-Null
    }
    foreach ($key in @('NoModify', 'NoRepair')) {
        New-ItemProperty -Path $UninstallKey -Name $key -Value 1 -PropertyType DWord -Force | Out-Null
    }
}

# 昇格した子プロセスから呼ばれたとき: 配置だけして終わる（失敗は窓が閉じる前に読めるよう待つ）
if ($Elevated) {
    try {
        Install-Privileged $PSScriptRoot
        exit 0
    } catch {
        Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
        Read-Host 'Enter キーで閉じます' | Out-Null
        exit 1
    }
}

try {
    Test-Platform
    if (-not $PSCommandPath) { throw 'スクリプトファイルとして実行してください（-File .\install.ps1）' }
    Test-Payload $PSScriptRoot

    $version = (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'VERSION') -TotalCount 1).Trim()
    Say "Nagi $version をインストールします（配置先: $InstallDir）"

    if (Test-Administrator) {
        Install-Privileged $PSScriptRoot
    } else {
        Say '管理者権限が必要です。ユーザーアカウント制御の確認が表示されます'
        $argumentList = '-NoProfile -ExecutionPolicy Bypass -File "{0}" -Elevated -InstallDir "{1}"' -f $PSCommandPath, $InstallDir
        $child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $argumentList -Verb RunAs -Wait -PassThru
        if ($child.ExitCode -ne 0) {
            throw "配置と登録に失敗しました（終了コード: $($child.ExitCode)）"
        }
    }
    Say "配置と入力方式の登録を終えました: $InstallDir"

    # Visual C++ 再頒布可能パッケージ（imserver が使う。ほとんどの PC に入っている）
    foreach ($runtime in @('vcruntime140.dll', 'msvcp140.dll')) {
        if (-not (Test-Path -LiteralPath (Join-Path $env:SystemRoot "System32\$runtime"))) {
            Say "警告: $runtime が見つかりません。https://aka.ms/vs/17/release/vc_redist.x64.exe を入れてください"
        }
    }

    $dataOk = $false
    if ($SkipData) {
        Say '辞書とモデルの取得を飛ばしました（-SkipData）'
    } else {
        $dataDir = Join-Path $env:APPDATA 'Nagi\data'
        Say '辞書とモデルを取得しています（約 150MB。取得済みのものは飛ばします）'
        # 標準出力は進捗の JSON（アプリ向け）なので捨て、標準エラーの人向けのログだけを表示する
        & (Join-Path $InstallDir $ExeName) --install-data --data-dir $dataDir --model $Model | Out-Null
        if ($LASTEXITCODE -eq 0) {
            $dataOk = $true
            Say "辞書とモデルを入れました: $dataDir"
        } else {
            Say '警告: 辞書とモデルを入れられませんでした。ネットワークを確認して、このスクリプトをもう一度実行してください'
        }
    }

    Write-Host ''
    Say "Nagi $version をインストールしました"
    Write-Host @'

次の手順:
  1. 設定 > 時刻と言語 > 言語と地域 > 日本語 >「言語のオプション」>「キーボードを追加する」で
     「Nagi」を選んでください。Win+Space で入力方式を切り替えられます。
  2. すでに開いているアプリでは、アプリを開き直すと反映されます。
     更新の場合は、サインアウト（または再起動）すると新しい版にそろいます。
  3. 辞書が無い間は、入力した読みがそのまま出ます。
'@
    if (-not $dataOk -and -not $SkipData) {
        Write-Host '  ※ 辞書の取得に失敗しています。もう一度このスクリプトを実行してください。'
    }
    exit 0
} catch {
    Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
