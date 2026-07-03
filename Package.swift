// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Mouseless",
    platforms: [
        .macOS(.v13)
    ],
    targets: [
        .executableTarget(
            name: "Mouseless",
            path: "Sources/Mouseless",
            swiftSettings: [
                .swiftLanguageMode(.v5)
            ]
        )
    ]
)
