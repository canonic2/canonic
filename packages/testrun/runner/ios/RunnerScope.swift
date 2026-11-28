// Owns bounded argument strings and explicit application/system scope resolution.
import Foundation
@preconcurrency import XCTest

extension TestrunRunnerTests {
    func string(_ arguments: [String: Any], _ key: String) throws -> String {
        guard let value = arguments[key] as? String, !value.isEmpty, value.utf8.count <= 65_536
        else {
            throw Failure(code: "INVALID_ARGUMENT", message: "Required string argument is invalid")
        }
        return value
    }
    func scope(_ arguments: [String: Any], requireForeground: Bool = false) throws
        -> XCUIApplication
    {
        guard let scope = arguments["scope"] as? [String: Any], let kind = scope["kind"] as? String
        else {
            throw Failure(code: "INVALID_ARGUMENT", message: "An explicit UI scope is required")
        }
        if kind == "system" { return XCUIApplication(bundleIdentifier: "com.apple.springboard") }
        guard kind == "app", let bundle = scope["bundleId"] as? String, !bundle.isEmpty else {
            throw Failure(
                code: "INVALID_ARGUMENT", message: "Application scope requires a bundle ID")
        }
        let app = XCUIApplication(bundleIdentifier: bundle)
        // Query process state before hierarchy/frame resolution: resolving a background
        // app can wait for XCTest idle and record an issue instead of throwing.
        if requireForeground, app.state != .runningForeground {
            throw Failure(code: "APP_BACKGROUNDED", message: "Scoped app is not foreground")
        }
        guard app.exists else {
            throw Failure(code: "UI_UNAVAILABLE", message: "Scoped application is not running")
        }
        return app
    }
}
