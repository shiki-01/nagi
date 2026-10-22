#!/bin/sh
# Nagi（macOS 用の日本語入力）のインストール・更新。
#   curl -fsSL https://nagi.shiki-01.com/install.sh | sh
#
# 最新版の情報（latest.json）を取得し、配布物（Developer ID 署名・公証済みの Nagi.app の zip）を
# SHA-256 と署名・公証で確かめてから ~/Library/Input Methods に置き、入力ソースに登録する。
# 管理者権限は使わない。入力内容や個人情報は送らない（取得するのは latest.json と zip だけ）。
#
# 環境変数:
#   NAGI_MANIFEST_URL  latest.json の URL（既定: https://nagi.shiki-01.com/updates/macos/latest.json）
set -eu

MANIFEST_URL="${NAGI_MANIFEST_URL:-https://nagi.shiki-01.com/updates/macos/latest.json}"
DEST_DIR="$HOME/Library/Input Methods"
APP="$DEST_DIR/Nagi.app"

say() { printf '%s\n' "nagi: $*"; }
fail() { printf '%s\n' "nagi: エラー: $*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || fail "macOS 専用です"
major=$(sw_vers -productVersion | cut -d. -f1)
[ "$major" -ge 13 ] || fail "macOS 13 以降が必要です（この Mac: $(sw_vers -productVersion)）"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "最新版の情報を取得しています"
curl -fsSL --proto '=https' "$MANIFEST_URL" -o "$tmp/latest.json" || fail "latest.json を取得できません: $MANIFEST_URL"
field() { plutil -extract "$1" raw -o - "$tmp/latest.json" 2>/dev/null || true; }
version=$(field version)
url=$(field download_url)
sha256=$(field sha256)
[ -n "$version" ] && [ -n "$url" ] && [ -n "$sha256" ] || fail "latest.json の内容が不正です"
case "$url" in https://*) ;; *) fail "配布物の URL が https ではありません: $url" ;; esac

say "Nagi $version をダウンロードしています"
curl -fL --proto '=https' --progress-bar "$url" -o "$tmp/Nagi.zip" || fail "ダウンロードに失敗しました: $url"
actual=$(shasum -a 256 "$tmp/Nagi.zip" | cut -d' ' -f1)
[ "$actual" = "$sha256" ] || fail "SHA-256 が一致しません（期待: $sha256 / 実際: $actual）"

ditto -x -k "$tmp/Nagi.zip" "$tmp/extracted" || fail "zip を展開できません"
new_app="$tmp/extracted/Nagi.app"
[ -d "$new_app" ] || fail "zip に Nagi.app が含まれていません"
codesign --verify --deep --strict "$new_app" 2>/dev/null || fail "Nagi.app の署名を検証できません"
spctl --assess --type execute "$new_app" 2>/dev/null || fail "Nagi.app が公証済みとして認められません（Gatekeeper）"

# 動作中の Nagi を止めてから置き換える（入力中の文字列は止める前に確定しておくこと）
if pgrep -x NagiIMEApp >/dev/null 2>&1; then
  say "動作中の Nagi を停止します"
  pkill -x NagiIMEApp || true
  sleep 1
fi
mkdir -p "$DEST_DIR"
if [ -d "$APP" ]; then
  mv "$APP" "$tmp/Nagi.app.old"
fi
ditto "$new_app" "$APP" || {
  [ -d "$tmp/Nagi.app.old" ] && mv "$tmp/Nagi.app.old" "$APP"
  fail "Nagi.app を配置できません: $APP"
}

say "入力ソースに登録しています"
"$APP/Contents/MacOS/NagiIMEApp" --register || say "警告: 登録に失敗しました。再ログイン後にシステム設定で追加してください"
killall TextInputMenuAgent >/dev/null 2>&1 || true

say "Nagi $version をインストールしました"
cat <<'EOF'

次の手順:
  1. システム設定 > キーボード > 入力ソース >「編集…」で「Nagi」（日本語）を追加してください。
     一覧に出ない場合は、いったんログアウトしてからもう一度お試しください。
  2. 変換に使う辞書・モデルは、~/Library/Application Support/com.shiki.nagi/data/ に置いてください
     （置き方: https://github.com/shiki-01/nagi/blob/main/docs/progress.md の「辞書・モデルと LLM 版の配置」）。
     辞書が無い間は、読みがそのまま入力されます。
  3. 以降の更新は、入力メニューの「アップデートを確認…」か、このコマンドの再実行で行えます。
EOF
