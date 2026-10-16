import UIKit
import XCTest

/// 入力欄の種類（`keyboardType` / `returnKeyType`）からレイアウトと改行キーの表示への対応表
final class InputTraitsMappingTests: XCTestCase {
    func testPreferredLayoutForKeyboardTypes() {
        let expected: [(UIKeyboardType, InputTraitsMapping.PreferredLayout?)] = [
            (.emailAddress, .alpha),
            (.URL, .alpha),
            (.asciiCapable, .alpha),
            (.numberPad, .numbers),
            (.decimalPad, .numbers),
            (.numbersAndPunctuation, .numbers),
            (.asciiCapableNumberPad, .numbers),
            (.phonePad, .numbers),
            (.namePhonePad, .numbers),
            // 指定なし（ユーザーが選んだレイアウトのまま）
            (.default, nil),
            (.twitter, nil),
            (.webSearch, nil),
        ]
        for (type, layout) in expected {
            XCTAssertEqual(InputTraitsMapping.preferredLayout(for: type), layout, "keyboardType=\(type.rawValue)")
        }
    }

    // 切替先は KeyboardModel.switchLayout の target（かな・英字は割り当て、数字は組み込みの id）
    func testPreferredLayoutTargetsAndModes() {
        XCTAssertEqual(InputTraitsMapping.PreferredLayout.kana.target, "kana")
        XCTAssertEqual(InputTraitsMapping.PreferredLayout.alpha.target, "alpha")
        XCTAssertEqual(InputTraitsMapping.PreferredLayout.numbers.target, "numbers")
        XCTAssertEqual(InputTraitsMapping.PreferredLayout.kana.mode, .kana)
        XCTAssertEqual(InputTraitsMapping.PreferredLayout.alpha.mode, .alpha)
        XCTAssertEqual(InputTraitsMapping.PreferredLayout.numbers.mode, .number)
    }

    func testReturnLabelForReturnKeyTypes() {
        let expected: [(UIReturnKeyType, String?)] = [
            (.search, "検索"),
            (.google, "検索"),
            (.yahoo, "検索"),
            (.go, "開く"),
            (.send, "送信"),
            (.done, "完了"),
            (.next, "次へ"),
            (.join, "参加"),
            (.route, "経路"),
            (.`continue`, "続ける"),
            (.emergencyCall, "緊急"),
            // 既定（改行）はレイアウトのラベルのまま
            (.default, nil),
        ]
        for (type, label) in expected {
            XCTAssertEqual(InputTraitsMapping.returnLabel(for: type), label, "returnKeyType=\(type.rawValue)")
        }
    }
}
