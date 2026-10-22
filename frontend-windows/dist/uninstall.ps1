<#
.SYNOPSIS
  Nagi（日本語入力）をアンインストールする。

.DESCRIPTION
    powershell -NoProfile -ExecutionPolicy Bypass -File .\uninstall.ps1 [-RemoveUserData]

  1. 管理者権限で regsvr32 /u により入力方式の登録を解除し、%ProgramFiles%\Nagi のファイルを削除する
     （管理者でなければ UAC の確認を出して、この手順だけを昇格して実行する）
  2. -RemoveUserData のときだけ、現在のユーザーのデータ（%APPDATA%\Nagi: 設定・学習・ユーザー辞書・
     取得した辞書とモデル）をすべて削除する。確認の入力が要る（-Force で省略）

  入力中のアプリが DLL を読み込んだままだと、その場では削除できない。そのファイルは名前を変えて退避し、
  次の再起動時に削除するよう予約する（サインアウトか再起動で完全に消える）。

.PARAMETER InstallDir
  配置先。既定は %ProgramFiles%\Nagi。

.PARAMETER RemoveUserData
  現在のユーザーの %APPDATA%\Nagi を削除する（学習データの全削除を含む）。

.PARAMETER Force
  -RemoveUserData の確認を省略する。

.PARAMETER Elevated
  内部用。昇格した子プロセスが登録解除とファイル削除だけを行う。
#>
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:ProgramFiles 'Nagi'),
    [switch]$RemoveUserData,
    [switch]$Force,
    [switch]$Elevated
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$DllName = 'nagi_windows_tsf.dll'
$UninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Nagi'

function Say([string]$Message) { Write-Host "nagi: $Message" }

function Test-Administrator {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-Regsvr32 {
    if ([Environment]::Is64BitProcess) {
        return (Join-Path $env:SystemRoot 'System32\regsvr32.exe')
    }
    return (Join-Path $env:SystemRoot 'Sysnative\regsvr32.exe')
}

# 次の再起動時に削除するよう予約する（MOVEFILE_DELAY_UNTIL_REBOOT = 4）
function Register-DeleteOnReboot([string]$Path) {
    if (-not ('Nagi.Native' -as [type])) {
        Add-Type -Namespace Nagi -Name Native -MemberDefinition @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern bool MoveFileEx(string lpExistingFileName, string lpNewFileName, int dwFlags);
'@
    }
    [void][Nagi.Native]::MoveFileEx($Path, $null, 4)
}

# 削除する。使用中で消せなければ名前を変えて退避し、再起動時の削除を予約する。戻り値: その場で消えたか
function Remove-FileOrSchedule([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return $true }
    try {
        Remove-Item -LiteralPath $Path -Force
        return $true
    } catch {
        $moved = "$Path.old"
        try {
            Remove-Item -LiteralPath $moved -Force -ErrorAction SilentlyContinue
            Move-Item -LiteralPath $Path -Destination $moved -Force
            Register-DeleteOnReboot $moved
        } catch {
            Register-DeleteOnReboot $Path
        }
        return $false
    }
}

# 管理者権限が要る手順: 登録解除・ファイル削除・「アプリと機能」の項目の削除
function Uninstall-Privileged {
    $dll = Join-Path $InstallDir $DllName
    $pending = $false
    if (Test-Path -LiteralPath $dll) {
        $process = Start-Process -FilePath (Get-Regsvr32) -ArgumentList @('/u', '/s', ('"{0}"' -f $dll)) -Wait -PassThru
        if ($process.ExitCode -ne 0) {
            Say "警告: regsvr32 /u が終了コード $($process.ExitCode) で終わりました。登録が残っている場合は、設定の言語オプションから Nagi を削除してください"
        }
    } else {
        Say "DLL が見つかりません（登録解除を飛ばします）: $dll"
    }

    if (Test-Path -LiteralPath $InstallDir) {
        # 子ディレクトリは作らない前提。念のため再帰で数える
        $files = Get-ChildItem -LiteralPath $InstallDir -Force -File -Recurse -ErrorAction SilentlyContinue
        foreach ($file in $files) {
            if (-not (Remove-FileOrSchedule $file.FullName)) { $pending = $true }
        }
        # 退避したファイルが残っている間はディレクトリを消せない（再起動時の削除に任せる）
        if (-not $pending) {
            Remove-Item -LiteralPath $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
        } else {
            Register-DeleteOnReboot $InstallDir
        }
    }
    Remove-Item -Path $UninstallKey -Recurse -Force -ErrorAction SilentlyContinue
    if ($pending) {
        Say '使用中のファイルは再起動時に削除されます。サインアウトか再起動をしてください'
    }
}

if ($Elevated) {
    try {
        Uninstall-Privileged
        exit 0
    } catch {
        Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
        Read-Host 'Enter キーで閉じます' | Out-Null
        exit 1
    }
}

try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows 専用です' }
    if (-not $PSCommandPath) { throw 'スクリプトファイルとして実行してください（-File .\uninstall.ps1）' }

    $userDir = Join-Path $env:APPDATA 'Nagi'
    if ($RemoveUserData -and -not $Force) {
        Write-Host "次のフォルダをすべて削除します（設定・学習データ・ユーザー辞書・取得した辞書とモデル）:"
        Write-Host "  $userDir"
        $answer = Read-Host '本当に削除しますか? 削除するには yes と入力してください'
        if ($answer -ne 'yes') {
            Say '中止しました（何も変更していません）'
            exit 1
        }
    }

    Say "Nagi をアンインストールします（$InstallDir）"
    if (Test-Administrator) {
        Uninstall-Privileged
    } else {
        Say '管理者権限が必要です。ユーザーアカウント制御の確認が表示されます'
        $argumentList = '-NoProfile -ExecutionPolicy Bypass -File "{0}" -Elevated -InstallDir "{1}"' -f $PSCommandPath, $InstallDir
        $child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $argumentList -Verb RunAs -Wait -PassThru
        if ($child.ExitCode -ne 0) {
            throw "登録解除とファイルの削除に失敗しました（終了コード: $($child.ExitCode)）"
        }
    }

    if ($RemoveUserData) {
        # 動作中のエンジンが学習データを書き戻さないよう、このユーザーの imserver を止めてから消す
        Get-Process -Name 'imserver' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 500
        if (Test-Path -LiteralPath $userDir) {
            Remove-Item -LiteralPath $userDir -Recurse -Force
            Say "ユーザーデータを削除しました: $userDir"
        } else {
            Say "ユーザーデータはありません: $userDir"
        }
    } else {
        Say "ユーザーデータ（設定・学習・辞書）は残しています: $userDir（削除するには -RemoveUserData）"
    }
    Say 'アンインストールしました。入力方式の一覧への反映は、サインアウトか再起動後になることがあります'
    exit 0
} catch {
    Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
