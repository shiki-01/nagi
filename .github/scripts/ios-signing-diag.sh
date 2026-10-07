#!/usr/bin/env bash
# ios-spike の署名診断。アプリと各 .appex の実行ファイルについて、署名・Mach-O 構造・ハッシュを出力する。
# 使い方: ios-signing-diag.sh <label> <app ディレクトリ> [<出力ファイル>]
# 診断用なので、個々のコマンドが失敗しても止めない（常に exit 0）。
set -u

label="${1:?label}"
app="${2:?app dir}"
out="${3:-}"

run() {
  echo "\$ $*"
  "$@" 2>&1 || echo "(exit $?)"
}

diag() {
  echo "===== [$label] ====="
  echo "ldid: $(command -v ldid || echo none)"
  run ldid --version
  echo
  echo "--- bundle ---"
  run ls -l "$app"
  run ls -l "$app"/PlugIns

  bins=("$app/NagiSpike")
  for appex in "$app"/PlugIns/*.appex; do
    [ -d "$appex" ] || continue
    exe=$(plutil -extract CFBundleExecutable raw -o - "$appex/Info.plist" 2>/dev/null || basename "$appex" .appex)
    echo "appex: $appex CFBundleExecutable=$exe"
    run plutil -p "$appex/Info.plist"
    bins+=("$appex/$exe")
  done

  for bin in "${bins[@]}"; do
    echo
    echo "--- $bin ---"
    if [ ! -f "$bin" ]; then
      echo "実行ファイルが存在しません"
      continue
    fi
    run ls -l "$bin"
    run shasum -a 256 "$bin"
    run file "$bin"
    run lipo -info "$bin"
    echo "[ldid -h]"
    run ldid -h "$bin"
    echo "[ldid -e]"
    run ldid -e "$bin"
    echo "[codesign -dvvv]"
    run codesign -dvvv "$bin"
    echo "[codesign --verify --strict -vv]"
    run codesign --verify --strict -vv "$bin"
    echo "[otool -hv]"
    run otool -hv "$bin"
    echo "[otool -l (code signature / entry / segments)]"
    otool -l "$bin" 2>&1 | grep -A4 -E 'LC_CODE_SIGNATURE|LC_MAIN|LC_UNIXTHREAD|LC_DYLD_CHAINED_FIXUPS|LC_DYLD_EXPORTS_TRIE' || true
    otool -l "$bin" 2>&1 | grep -E 'segname|vmsize|fileoff|filesize|flags|initprot' || true
    echo "[otool -L]"
    run otool -L "$bin"
  done
}

if [ -n "$out" ]; then
  mkdir -p "$(dirname "$out")"
  diag 2>&1 | tee "$out"
else
  diag
fi
exit 0
