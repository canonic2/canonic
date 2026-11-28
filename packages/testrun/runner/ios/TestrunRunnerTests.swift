import Foundation
@preconcurrency import Network
import UIKit
@preconcurrency import XCTest

// Typed, authenticated USB automation. No script evaluator or request-body logs.
@MainActor
// Nonisolated ingress touches only immutable request/configuration bytes and the
// locked admission owner. All XCTest/session mutation remains on MainActor.
final class TestrunRunnerTests: SBRunnerIssueCase, @unchecked Sendable {
    struct Failure: Error {
        let code: String
        let message: String
    }
    struct Configuration: Decodable {
        let sessionId: String
        let token: String
        let port: UInt16
        let expiresAt: Date
    }
    var configuration: Configuration!
    private var listener: NWListener?
    var stopped = false
    var snapshots: [String: (Date, Data, [String: XCUIElement])] = [:]
    var commands: [String: (Data, [String: Any])] = [:]
    nonisolated let requests = RunnerRequests()
    var currentRequest: RunnerRequests.Ticket?
    private nonisolated let network = DispatchQueue(label: "com.canonic.testrun.runner.network")

    func testServeOwnedSession() throws {
        // Record issues in XCTest and reply to the active RPC before teardown.
        continueAfterFailure = true
        var runtimeError: NSError?
        guard SBInstallNonWaitingQuiescence(&runtimeError) else {
            throw Failure(
                code: "RUNNER_RUNTIME_UNSUPPORTED",
                message: runtimeError?.localizedDescription ?? "Unsupported XCTest idle-wait runtime")
        }
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let path = documents.appendingPathComponent("testrun-session.json")
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        configuration = try decoder.decode(Configuration.self, from: Data(contentsOf: path))
        try FileManager.default.removeItem(at: path)
        guard configuration.token.utf8.count == 64, configuration.expiresAt > Date(),
            configuration.port > 0
        else {
            throw Failure(
                code: "INVALID_CONFIGURATION", message: "Runner session configuration is invalid")
        }
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(
            host: "127.0.0.1", port: NWEndpoint.Port(rawValue: configuration.port)!)
        let listener = try NWListener(using: parameters)
        self.listener = listener
        let sessionConfiguration = configuration!
        listener.newConnectionHandler = { [weak self] connection in
            connection.start(queue: self?.network ?? .global())
            self?.receiveRequest(connection, configuration: sessionConfiguration)
            (self?.network ?? .global()).asyncAfter(deadline: .now() + 30) { connection.cancel() }
        }
        listener.start(queue: network)
        requests.touch()
        while !stopped && Date() < configuration.expiresAt
            && !requests.idleExpired
        { RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.02)) }
        listener.cancel()
        snapshots.removeAll()
        commands.removeAll()
        configuration = nil
    }
    func execute(_ action: String, arguments: [String: Any]) throws -> ([String: Any], Data) {
        let admitted = ProcessInfo.processInfo.systemUptime
        // Validate against a fresh device image before dispatch, without PNG encoding
        // or transferring a full screenshot over USB for every gesture.
        if let display = arguments["display"] as? [String: Any] {
            let image = XCUIScreen.main.screenshot().image
            guard let pixels = image.cgImage, display["displayId"] as? String == "main",
                display["geometryVersion"] as? String == "\(pixels.width)x\(pixels.height)"
            else {
                throw Failure(
                    code: "STALE_GEOMETRY",
                    message: "Display geometry changed; capture a fresh screenshot")
            }
        }
        let geometryReady = ProcessInfo.processInfo.systemUptime
        switch action {
        case "screenshot", "ui_snapshot":
            return try observe(action, arguments: arguments)
        case "tap", "long_press", "swipe":
            return try performGesture(
                action, arguments: arguments, admitted: admitted, geometryReady: geometryReady)
        case "type_text":
            return try typeText(arguments)
        case "app_launch":
            let app = XCUIApplication(bundleIdentifier: try string(arguments, "bundleId"))
            app.launchArguments = arguments["args"] as? [String] ?? []
            if app.exists { app.activate() } else { app.launch() }
            snapshots.removeAll()
            return (["running": app.exists], Data())
        case "app_terminate":
            XCUIApplication(bundleIdentifier: try string(arguments, "bundleId")).terminate()
            snapshots.removeAll()
            return (["terminated": true], Data())
        default:
            throw Failure(
                code: "CAPABILITY_UNAVAILABLE", message: "Runner operation is not supported")
        }
    }
}
