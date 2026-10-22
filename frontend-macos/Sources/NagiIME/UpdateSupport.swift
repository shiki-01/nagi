import Foundation

#if canImport(CryptoKit)
import CryptoKit
#endif

/// アップデートの `latest.json`（core の `update_check::Manifest` と同じ形）
public struct UpdateManifest: Decodable, Equatable {
    public var version: String
    public var notes_url: String?
    public var download_url: String?
    public var sha256: String?
    public var signature: String?

    public init(
        version: String, notes_url: String? = nil, download_url: String? = nil,
        sha256: String? = nil, signature: String? = nil
    ) {
        self.version = version
        self.notes_url = notes_url
        self.download_url = download_url
        self.sha256 = sha256
        self.signature = signature
    }
}

/// バージョン比較（core の `update_check::parse_version` と同じ規則: "x.y.z"、読めない要素は 0）
public enum UpdateVersion {
    public static func parse(_ s: String) -> [Int] {
        var parts = s.split(separator: ".", omittingEmptySubsequences: false).prefix(3).map {
            Int($0) ?? 0
        }
        while parts.count < 3 { parts.append(0) }
        return parts
    }

    public static func isNewer(_ candidate: String, than current: String) -> Bool {
        parse(current).lexicographicallyPrecedes(parse(candidate))
    }
}

/// 配布物の検証失敗の理由（利用者に見せる文言を持つ）
public enum UpdateVerificationError: Error, Equatable {
    case missingField(String)
    case invalidHex(String)
    case checksumMismatch
    case signatureMismatch
    case unsupported

    public var message: String {
        switch self {
        case .missingField(let f): return "アップデート情報に \(f) がありません"
        case .invalidHex(let f): return "アップデート情報の \(f) が不正です"
        case .checksumMismatch: return "ダウンロードした配布物の SHA-256 が一致しません"
        case .signatureMismatch: return "ダウンロードした配布物の署名を検証できません"
        case .unsupported: return "この環境では署名を検証できません"
        }
    }
}

/// 配布物の SHA-256 と ed25519 署名を検証する（core の `update_check::verify_bytes` と同じ方式）
public enum UpdateVerifier {
    /// 16 進文字列 → バイト列（大文字小文字を区別しない。奇数長・不正な文字は nil）
    public static func decodeHex(_ s: String) -> Data? {
        let chars = Array(s.utf8)
        guard chars.count % 2 == 0 else { return nil }
        var out = Data(capacity: chars.count / 2)
        var i = 0
        while i < chars.count {
            guard let hi = nibble(chars[i]), let lo = nibble(chars[i + 1]) else { return nil }
            out.append(hi << 4 | lo)
            i += 2
        }
        return out
    }

    private static func nibble(_ c: UInt8) -> UInt8? {
        switch c {
        case UInt8(ascii: "0")...UInt8(ascii: "9"): return c - UInt8(ascii: "0")
        case UInt8(ascii: "a")...UInt8(ascii: "f"): return c - UInt8(ascii: "a") + 10
        case UInt8(ascii: "A")...UInt8(ascii: "F"): return c - UInt8(ascii: "A") + 10
        default: return nil
        }
    }

    public static func verify(_ data: Data, manifest: UpdateManifest, publicKeyHex: String) throws {
        #if canImport(CryptoKit)
        guard let sha = manifest.sha256 else { throw UpdateVerificationError.missingField("sha256") }
        guard let sigHex = manifest.signature else {
            throw UpdateVerificationError.missingField("signature")
        }
        let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        guard digest == sha.lowercased() else { throw UpdateVerificationError.checksumMismatch }
        guard let signature = decodeHex(sigHex), signature.count == 64 else {
            throw UpdateVerificationError.invalidHex("signature")
        }
        guard let keyData = decodeHex(publicKeyHex.trimmingCharacters(in: .whitespacesAndNewlines)),
            let key = try? Curve25519.Signing.PublicKey(rawRepresentation: keyData)
        else { throw UpdateVerificationError.invalidHex("公開鍵") }
        guard key.isValidSignature(signature, for: data) else {
            throw UpdateVerificationError.signatureMismatch
        }
        #else
        throw UpdateVerificationError.unsupported
        #endif
    }
}
