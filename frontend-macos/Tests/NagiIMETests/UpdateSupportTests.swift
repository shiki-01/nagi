import Foundation
import NagiClient
import XCTest

@testable import NagiIME

final class UpdateSupportTests: XCTestCase {
    /// `cargo xtask sign-update`（ed25519-dalek）で、使い捨ての seed（"11…11"）から作った既知の値
    private let data = Data("nagi-update-test".utf8)
    private let publicKey = "d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737"
    private let manifest = UpdateManifest(
        version: "9.9.9", download_url: "https://example.invalid/f.zip",
        sha256: "0cf07602eb15bef6ab0671bff2178c7bc178b15a3d48785cfa8ebe7adbc32873",
        signature:
            "08d602c3097b215cc64f1aa6e3c8d36af7c444fe0d40e30bb1fd52363ac490db73091166722ac310bba021a7a5c6138e8b68f092d1a2dba1abe169b613a8220a"
    )

    func testVersionComparisonMatchesCore() {
        XCTAssertTrue(UpdateVersion.isNewer("0.1.0", than: "0.0.9"))
        XCTAssertTrue(UpdateVersion.isNewer("1.0.0", than: "0.99.99"))
        XCTAssertFalse(UpdateVersion.isNewer("1.2.3", than: "1.2.3"))
        XCTAssertFalse(UpdateVersion.isNewer("1.2", than: "1.2.0"))
        XCTAssertFalse(UpdateVersion.isNewer("0.9.0", than: "1.0.0"))
        XCTAssertEqual(UpdateVersion.parse("bad"), [0, 0, 0])
    }

    func testHexDecoding() {
        XCTAssertEqual(UpdateVerifier.decodeHex("00ffA0"), Data([0x00, 0xff, 0xa0]))
        XCTAssertNil(UpdateVerifier.decodeHex("abc"))
        XCTAssertNil(UpdateVerifier.decodeHex("zz"))
    }

    func testVerifiesSignatureMadeByXtask() throws {
        XCTAssertNoThrow(try UpdateVerifier.verify(data, manifest: manifest, publicKeyHex: publicKey))
    }

    func testRejectsTamperedData() {
        XCTAssertThrowsError(
            try UpdateVerifier.verify(Data("nagi-update-tesT".utf8), manifest: manifest, publicKeyHex: publicKey)
        ) { XCTAssertEqual($0 as? UpdateVerificationError, .checksumMismatch) }
    }

    func testRejectsWrongKey() {
        let other = String(repeating: "ab", count: 32)
        XCTAssertThrowsError(
            try UpdateVerifier.verify(data, manifest: manifest, publicKeyHex: other)
        ) { error in
            let e = error as? UpdateVerificationError
            XCTAssertTrue(e == .signatureMismatch || e == .invalidHex("公開鍵"))
        }
    }

    func testRequiresChecksumAndSignature() {
        var m = manifest
        m.signature = nil
        XCTAssertThrowsError(try UpdateVerifier.verify(data, manifest: m, publicKeyHex: publicKey)) {
            XCTAssertEqual($0 as? UpdateVerificationError, .missingField("signature"))
        }
    }

    func testUpdateAvailableNoticeIsShown() {
        let n = NotifyParams(level: .info, message: "新しいバージョン 0.2.0 が利用可能です", code: "update_available")
        XCTAssertEqual(
            StatusMessages.toast(for: .notify(n)),
            "新しいバージョン 0.2.0 が利用可能です。入力メニューの「アップデートを確認…」から更新できます")
    }
}
