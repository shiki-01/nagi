// swift-tools-version:5.9
import PackageDescription

// Xcode プロジェクトは使わず SwiftPM のみで構成する。
// NagiProtocol / NagiClient は Foundation のみに依存し、Linux でもビルド・テストできる。
let package = Package(
    name: "NagiFrontend",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "NagiClient", targets: ["NagiClient"]),
        .library(name: "NagiIME", targets: ["NagiIME"]),
    ],
    targets: [
        // typeshare 生成物（core/protocol の Rust 型から生成。編集禁止）
        .target(
            name: "NagiProtocol",
            path: "Sources/Generated"
        ),
        .target(
            name: "NagiClient",
            dependencies: ["NagiProtocol"],
            path: "Sources/NagiClient"
        ),
        // IMKit 層。プレースホルダ（中身は `#if os(macOS)` で囲む）
        .target(
            name: "NagiIME",
            dependencies: ["NagiClient"],
            path: "Sources/NagiIME"
        ),
        .testTarget(
            name: "NagiClientTests",
            dependencies: ["NagiClient"],
            path: "Tests/NagiClientTests"
        ),
    ]
)
