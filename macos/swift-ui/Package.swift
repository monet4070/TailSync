// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "TailSync",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "TailSync",
            path: "Sources/TailSync"
        ),
        .executableTarget(
            name: "TailSyncPreview",
            path: "Sources/TailSyncPreview"
        ),
        .testTarget(
            name: "TailSyncTests",
            dependencies: ["TailSync"],
            path: "Tests/TailSyncTests"
        ),
    ]
)
