// swift-tools-version:6.0
// jl-speech: the macOS speech helper JELearner's server calls (docs/decisions.md §14).
import PackageDescription

let package = Package(
    name: "jl-speech",
    platforms: [.macOS("26.0")],
    targets: [
        .executableTarget(name: "jl-speech", path: "Sources/jl-speech"),
    ]
)
