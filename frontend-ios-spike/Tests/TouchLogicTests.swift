import XCTest

final class TouchLogicTests: XCTestCase {
    // MARK: SpaceSwipeTracker

    func testTinyMovementDoesNothingAndStaysATap() {
        var t = SpaceSwipeTracker(step: 14)
        XCTAssertEqual(t.update(translationWidth: 5), 0)
        XCTAssertEqual(t.update(translationWidth: -13.9), 0)
        XCTAssertFalse(t.isActive)
    }

    func testEntersCursorModeAtThresholdAndMovesProportionally() {
        var t = SpaceSwipeTracker(step: 14)
        XCTAssertEqual(t.update(translationWidth: 14), 1)
        XCTAssertTrue(t.isActive)
        XCTAssertEqual(t.update(translationWidth: 20), 0)
        XCTAssertEqual(t.update(translationWidth: 43), 2)   // 合計 3 文字
        XCTAssertEqual(t.update(translationWidth: 100), 4)  // 合計 7 文字
    }

    func testLeftSwipeIsNegative() {
        var t = SpaceSwipeTracker(step: 10)
        XCTAssertEqual(t.update(translationWidth: -25), -2)
        XCTAssertEqual(t.update(translationWidth: -39), -1)
    }

    func testReturningTheFingerMovesBack() {
        var t = SpaceSwipeTracker(step: 10)
        XCTAssertEqual(t.update(translationWidth: 30), 3)
        XCTAssertEqual(t.update(translationWidth: 5), -3)  // 合計 0 文字（0.5 → 0）まで戻る
        XCTAssertTrue(t.isActive)  // いったん入ったら、元の位置に戻ってもスペースは入れない
    }

    func testStepIsClampedAndNonFiniteIgnored() {
        var t = SpaceSwipeTracker(step: 0)
        XCTAssertEqual(t.step, 1)
        XCTAssertEqual(t.update(translationWidth: .nan), 0)
        XCTAssertFalse(t.isActive)
    }

    // MARK: WordBoundary

    func testDeleteCountEmpty() {
        XCTAssertEqual(WordBoundary.deleteCount(before: ""), 0)
    }

    func testKanjiRunThenKana() {
        XCTAssertEqual(WordBoundary.deleteCount(before: "今日は天気"), 2)
        XCTAssertEqual(WordBoundary.deleteCount(before: "今日は"), 1)
        XCTAssertEqual(WordBoundary.deleteCount(before: "これはテスト"), 3)
        XCTAssertEqual(WordBoundary.deleteCount(before: "あした、そうだ"), 3)
    }

    func testLatinWordWithTrailingSpaces() {
        XCTAssertEqual(WordBoundary.deleteCount(before: "hello world"), 5)
        XCTAssertEqual(WordBoundary.deleteCount(before: "hello world  "), 7)
        XCTAssertEqual(WordBoundary.deleteCount(before: "abc123"), 6)
    }

    func testPunctuationDeletesOne() {
        XCTAssertEqual(WordBoundary.deleteCount(before: "こんにちは。"), 1)
        XCTAssertEqual(WordBoundary.deleteCount(before: "a!!"), 1)
    }

    func testNewlineDeletesOne() {
        XCTAssertEqual(WordBoundary.deleteCount(before: "abc\n"), 1)
    }

    func testOnlySpacesAndSpaceBeforePunctuation() {
        XCTAssertEqual(WordBoundary.deleteCount(before: "   "), 3)
        XCTAssertEqual(WordBoundary.deleteCount(before: "あ。 "), 2)
    }

    func testCapAtMaxCount() {
        let long = String(repeating: "a", count: 200)
        XCTAssertEqual(WordBoundary.deleteCount(before: long), WordBoundary.maxCount)
    }

    func testKatakanaLongVowelMark() {
        XCTAssertEqual(WordBoundary.deleteCount(before: "あのコーヒー"), 4)
    }

    // MARK: FlickCancel

    func testFlickWithinTwoKeysIsNotCancelled() {
        // キー 40×50: 横 80・縦 100 まではフリックのまま
        XCTAssertFalse(FlickCancel.isCancelled(dx: 0, dy: 0, keyWidth: 40, keyHeight: 50))
        XCTAssertFalse(FlickCancel.isCancelled(dx: 80, dy: 0, keyWidth: 40, keyHeight: 50))
        XCTAssertFalse(FlickCancel.isCancelled(dx: -80, dy: -100, keyWidth: 40, keyHeight: 50))
    }

    func testFlickBeyondTwoKeysIsCancelled() {
        XCTAssertTrue(FlickCancel.isCancelled(dx: 80.5, dy: 0, keyWidth: 40, keyHeight: 50))
        XCTAssertTrue(FlickCancel.isCancelled(dx: -81, dy: 0, keyWidth: 40, keyHeight: 50))
        XCTAssertTrue(FlickCancel.isCancelled(dx: 0, dy: 101, keyWidth: 40, keyHeight: 50))
        XCTAssertTrue(FlickCancel.isCancelled(dx: 0, dy: -101, keyWidth: 40, keyHeight: 50))
    }

    func testFlickCancelIgnoresInvalidValues() {
        XCTAssertFalse(FlickCancel.isCancelled(dx: .nan, dy: 0, keyWidth: 40, keyHeight: 50))
        XCTAssertFalse(FlickCancel.isCancelled(dx: 0, dy: .infinity, keyWidth: 40, keyHeight: 50))
        XCTAssertFalse(FlickCancel.isCancelled(dx: 500, dy: 500, keyWidth: 0, keyHeight: 50))
        XCTAssertFalse(FlickCancel.isCancelled(dx: 500, dy: 500, keyWidth: 40, keyHeight: -1))
    }

    // MARK: LongPressCancel

    func testLongPressCancelBelowKey() {
        XCTAssertEqual(LongPressCancel.heightFactor, 0.3, accuracy: 1e-9)
        // キーの高さ 50: 下端 50 から 15 を超えたら取り消し
        XCTAssertFalse(LongPressCancel.isCancelled(y: 25, keyHeight: 50))
        XCTAssertFalse(LongPressCancel.isCancelled(y: 65, keyHeight: 50))
        XCTAssertTrue(LongPressCancel.isCancelled(y: 65.5, keyHeight: 50))
        XCTAssertFalse(LongPressCancel.isCancelled(y: .nan, keyHeight: 50))
        XCTAssertFalse(LongPressCancel.isCancelled(y: 999, keyHeight: 0))
    }

    func testLongPressCancelSideways() {
        // ポップアップ: 左端 -20・幅 120（項目 40 × 3）。左は -60 未満、右は 140 を超えたら取り消し
        func cancelled(_ x: Double, _ y: Double = 25) -> Bool {
            LongPressCancel.isCancelled(x: x, y: y, keyHeight: 50, popupLeft: -20, popupWidth: 120, itemWidth: 40)
        }
        XCTAssertFalse(cancelled(0))
        XCTAssertFalse(cancelled(-60))
        XCTAssertFalse(cancelled(140))
        XCTAssertTrue(cancelled(-60.5))
        XCTAssertTrue(cancelled(141))
        // 下へ外れたときも取り消し（横が範囲内でも）
        XCTAssertTrue(cancelled(0, 66))
        // 最下段のキーで下へずらせなくても、横へずらせば取り消せる
        XCTAssertTrue(cancelled(-100, 40))
        XCTAssertFalse(cancelled(.nan))
    }
}

final class ThemeTests: XCTestCase {
    func testInterfaceStyle() {
        XCTAssertEqual(NagiTheme.interfaceStyle("light"), .light)
        XCTAssertEqual(NagiTheme.interfaceStyle("dark"), .dark)
        XCTAssertEqual(NagiTheme.interfaceStyle("system"), .unspecified)
        XCTAssertEqual(NagiTheme.interfaceStyle("???"), .unspecified)
    }

    func testEveryAccentChoiceHasAColor() {
        let def = NagiIOSSettings.definition(for: NagiIOSSettings.Key.keyboardAccent)
        guard case .choice(let choices)? = def?.kind else { return XCTFail("choice ではない") }
        XCTAssertEqual(Set(choices.map { $0.value }), ["blue", "green", "orange", "pink", "gray"])
        let colors = choices.map { NagiTheme.accentUIColor($0.value) }
        for i in 0..<colors.count {
            for j in (i + 1)..<colors.count { XCTAssertNotEqual(colors[i], colors[j]) }
        }
        XCTAssertEqual(NagiTheme.accentUIColor("unknown"), NagiTheme.accentUIColor("blue"))
    }

    func testNewSettingsDefaultsAndRounding() {
        var s = NagiIOSSettings()
        XCTAssertTrue(s.spaceSwipe)
        XCTAssertEqual(s.spaceSwipeStepPt, 14, accuracy: 1e-9)
        XCTAssertFalse(s.backspaceSwipe)
        XCTAssertEqual(s.theme, "system")
        XCTAssertEqual(s.accent, "blue")
        XCTAssertTrue(s.pasteButton)
        XCTAssertEqual(s.soundMode, "compat")
        XCTAssertTrue(s.soundEnabled)
        XCTAssertEqual(s.commitMethod, "insert")
        s.set(NagiIOSSettings.Key.keyboardSound, .string("off"))
        XCTAssertFalse(s.soundEnabled)
        s.set(NagiIOSSettings.Key.keyboardSound, .string("system"))
        XCTAssertEqual(s.soundMode, "system")
        XCTAssertTrue(s.soundEnabled)
        s.set(NagiIOSSettings.Key.keyboardCommitMethod, .string("delete"))
        XCTAssertEqual(s.commitMethod, "delete")
        s.set(NagiIOSSettings.Key.keyboardCommitMethod, .string("replace"))
        XCTAssertEqual(s.commitMethod, "insert")
        s.set(NagiIOSSettings.Key.keyboardTheme, .string("dark"))
        XCTAssertEqual(s.theme, "dark")
        s.set(NagiIOSSettings.Key.keyboardTheme, .string("sepia"))
        XCTAssertEqual(s.theme, "system")
        s.set(NagiIOSSettings.Key.keyboardAccent, .string("red"))
        XCTAssertEqual(s.accent, "blue")
        s.set(NagiIOSSettings.Key.spaceSwipeStepPt, .number(1))
        XCTAssertEqual(s.spaceSwipeStepPt, 4, accuracy: 1e-9)
        s.set(NagiIOSSettings.Key.spaceSwipeStepPt, .number(99))
        XCTAssertEqual(s.spaceSwipeStepPt, 40, accuracy: 1e-9)
    }

    func testSimpleModeShowsOnlyThemeAndAccentAmongNewItems() {
        let newKeys: Set<String> = [
            NagiIOSSettings.Key.spaceSwipe, NagiIOSSettings.Key.spaceSwipeStepPt, NagiIOSSettings.Key.backspaceSwipe,
            NagiIOSSettings.Key.keyboardTheme, NagiIOSSettings.Key.keyboardAccent, NagiIOSSettings.Key.candbarPasteButton,
        ]
        let simple = NagiIOSSettings.definitions.filter { newKeys.contains($0.key) && $0.isSimple }.map { $0.key }
        XCTAssertEqual(Set(simple), [NagiIOSSettings.Key.keyboardTheme, NagiIOSSettings.Key.keyboardAccent])
    }
}
