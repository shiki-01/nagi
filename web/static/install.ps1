# Nagi (Japanese IME) installer for Windows 10 22H2+ / 11 (x64).
#   irm https://nagi.shiki-01.com/install.ps1 | iex
#
# Fetches latest.json, downloads the release zip, verifies its SHA-256, extracts it to a temporary
# folder and runs the install.ps1 inside (which asks for administrator rights via UAC, registers the
# input method and downloads the dictionary and model). Files fetched with Invoke-WebRequest carry no
# Mark-of-the-Web, so SmartScreen does not warn (the trust is in this site and HTTPS, as with the
# macOS install.sh). No input or personal data is sent.
#
# This file is ASCII on purpose: `irm | iex` decodes the response with the default code page.
# The installer inside the zip prints Japanese messages.
#
# Environment variables:
#   NAGI_MANIFEST_URL  latest.json URL (default: https://nagi.shiki-01.com/updates/windows/latest.json)
#   NAGI_SKIP_DATA     1 = do not download the dictionary and model

function Install-Nagi {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # progress rendering makes Windows PowerShell 5.1 downloads very slow
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $manifestUrl = $env:NAGI_MANIFEST_URL
    if (-not $manifestUrl) { $manifestUrl = 'https://nagi.shiki-01.com/updates/windows/latest.json' }
    $work = Join-Path ([IO.Path]::GetTempPath()) ('nagi-install-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
        Write-Host 'nagi: fetching latest.json'
        $manifest = Invoke-RestMethod -UseBasicParsing -Uri $manifestUrl
        if (-not ($manifest.version -and $manifest.download_url -and $manifest.sha256)) {
            throw 'latest.json is invalid'
        }
        if (-not ([string]$manifest.download_url).StartsWith('https://')) {
            throw "download_url is not https: $($manifest.download_url)"
        }

        Write-Host "nagi: downloading Nagi $($manifest.version)"
        $zip = Join-Path $work 'Nagi.zip'
        Invoke-WebRequest -UseBasicParsing -Uri $manifest.download_url -OutFile $zip
        $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne ([string]$manifest.sha256).ToLowerInvariant()) {
            throw "SHA-256 mismatch (expected $($manifest.sha256), got $actual)"
        }

        $extracted = Join-Path $work 'extracted'
        Expand-Archive -LiteralPath $zip -DestinationPath $extracted
        $script = Get-ChildItem -LiteralPath $extracted -Recurse -Filter 'install.ps1' | Select-Object -First 1
        if (-not $script) { throw 'install.ps1 is missing from the zip' }

        $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $script.FullName)
        if ($env:NAGI_SKIP_DATA -eq '1') { $arguments += '-SkipData' }
        & (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') @arguments
        if ($LASTEXITCODE -ne 0) { throw "install.ps1 failed (exit code $LASTEXITCODE)" }
    } catch {
        Write-Host "nagi: error: $($_.Exception.Message)" -ForegroundColor Red
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Install-Nagi
