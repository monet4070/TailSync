// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "TrayHelper",
    platforms: [.macOS(.v12)],
    targets: [
        .executableTarget(
            name: "TrayHelper",
            path: "Sources"
        ),
    ]
)
