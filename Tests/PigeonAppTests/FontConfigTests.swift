import XCTest
@testable import PigeonApp

final class FontConfigTests: XCTestCase {
    func testDefaultsAreSFMono13At125() {
        let config = FontConfig(toml: "")
        XCTAssertEqual(config, FontConfig(family: "", size: 13, lineHeight: 1.25))
    }

    func testReadsFamilySizeAndLineHeight() {
        let config = FontConfig(toml: """
        # pigeon
        font-family = "JetBrains Mono"   # the one I like
        font-size = 14.5
        line-height = 1.3
        """)
        XCTAssertEqual(config.family, "JetBrains Mono")
        XCTAssertEqual(config.size, 14.5)
        XCTAssertEqual(config.lineHeight, 1.3)
    }

    func testMalformedOrOutOfRangeValuesKeepTheDefaults() {
        let config = FontConfig(toml: """
        font-family = JetBrains Mono
        font-size = 400
        line-height = "tall"
        [section]
        unknown = 3
        garbage line
        font-family = "bad\u{07}name"
        """)
        XCTAssertEqual(config, FontConfig())
    }

    func testAHashInsideTheNameIsNotAComment() {
        XCTAssertEqual(FontConfig(toml: #"font-family = "Mono #2""#).family, "Mono #2")
    }

    func testAMissingFileIsTheDefault() {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID()).toml")
        XCTAssertEqual(FontConfig.load(from: url), FontConfig())
    }
}
