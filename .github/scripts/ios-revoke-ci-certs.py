#!/usr/bin/env python3
"""CI が作った開発証明書（"Created via API"）を App Store Connect API で失効させる。

背景: CI の Mac は毎回まっさらで、署名の自動管理（-allowProvisioningUpdates）が開発証明書を
毎回新しく作る。残ったままだと上限に達し、アーカイブが "maximum number of certificates" で失敗する。
ここで消すのは名前に "Created via API" を含む開発証明書だけ（人が Xcode で作った証明書は対象外）。
配布証明書（クラウド管理）には触らない。

環境変数: ASC_KEY_ID / ASC_ISSUER_ID / ASC_KEY_PATH（AuthKey_XXXX.p8 のパス）
引数: --dry-run  一覧だけ表示して失効させない
"""
import base64
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

API = "https://api.appstoreconnect.apple.com"
MARKER = "Created via API"


def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def der_to_raw(der: bytes, size: int = 32) -> bytes:
    """openssl が返す DER の ECDSA 署名（SEQUENCE { INTEGER r, INTEGER s }）を、JWT の r||s にする"""
    assert der[0] == 0x30
    pos = 2 if der[1] < 0x80 else 2 + (der[1] & 0x7F)
    parts = []
    for _ in range(2):
        assert der[pos] == 0x02
        length = der[pos + 1]
        value = der[pos + 2 : pos + 2 + length]
        parts.append(value.lstrip(b"\x00").rjust(size, b"\x00"))
        pos += 2 + length
    return parts[0] + parts[1]


def make_token(key_id: str, issuer_id: str, key_path: str) -> str:
    header = {"alg": "ES256", "kid": key_id, "typ": "JWT"}
    now = int(time.time())
    payload = {"iss": issuer_id, "iat": now, "exp": now + 600, "aud": "appstoreconnect-v1"}
    signing_input = (
        b64url(json.dumps(header, separators=(",", ":")).encode())
        + "."
        + b64url(json.dumps(payload, separators=(",", ":")).encode())
    )
    der = subprocess.run(
        ["openssl", "dgst", "-sha256", "-sign", key_path],
        input=signing_input.encode(),
        capture_output=True,
        check=True,
    ).stdout
    return signing_input + "." + b64url(der_to_raw(der))


def request(method: str, path: str, token: str):
    req = urllib.request.Request(API + path, method=method)
    req.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req, timeout=60) as res:
            body = res.read()
            return json.loads(body) if body else {}
    except urllib.error.HTTPError as e:
        detail = e.read().decode(errors="replace")
        sys.exit(f"API エラー {e.code} {method} {path}: {detail}")


def main() -> None:
    dry_run = "--dry-run" in sys.argv
    token = make_token(
        os.environ["ASC_KEY_ID"], os.environ["ASC_ISSUER_ID"], os.environ["ASC_KEY_PATH"]
    )
    listing = request(
        "GET",
        "/v1/certificates?filter[certificateType]=IOS_DEVELOPMENT,DEVELOPMENT&limit=200",
        token,
    )
    certs = listing.get("data", [])
    print(f"開発証明書: {len(certs)} 件")
    targets = []
    for c in certs:
        a = c.get("attributes", {})
        label = f"{a.get('name')} / {a.get('displayName')} / {a.get('certificateType')} / 期限 {a.get('expirationDate')}"
        is_ci = MARKER in (a.get("name") or "") or MARKER in (a.get("displayName") or "")
        print(f"  {'[CI 用]' if is_ci else '[対象外]'} {c['id']} {label}")
        if is_ci:
            targets.append(c["id"])
    if dry_run:
        print(f"dry-run: {len(targets)} 件を失効させる予定（実行しない）")
        return
    for cert_id in targets:
        request("DELETE", f"/v1/certificates/{cert_id}", token)
        print(f"失効: {cert_id}")
    print(f"失効した証明書: {len(targets)} 件")


if __name__ == "__main__":
    main()
