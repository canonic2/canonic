// Replaces only XCTest execution; authentication, sockets and admission are production code.
import Foundation
import Network

@MainActor
final class TestrunRunnerTests: @unchecked Sendable {
    struct Failure: Error { let code: String; let message: String }
    struct Configuration {
        let sessionId: String; let token: String; let expiresAt: Date
    }
    final class Issues {
        var count = 0
        func beginCommand() -> Int { 0 }
        func beginObservation() -> Bool { true }
        func finishObservation() {}
        func finishCommand(withUncertainOutcome: Bool) -> Int { 0 }
    }
    nonisolated let requests = RunnerRequests(inputGrace: 2)
    var currentRequest: RunnerRequests.Ticket?
    let inputIssues = Issues()
    var commands: [String: (Data, [String: Any])] = [:]
    var stopped = false
    private var mutations = 0

    func execute(_ action: String, arguments: [String: Any]) throws -> ([String: Any], Data) {
        if action == "ui_snapshot" {
            Thread.sleep(forTimeInterval: arguments["fixtureDelay"] as? Double ?? 0)
            return (["snapshotId": UUID().uuidString], Data())
        }
        if action == "app_launch" {
            Thread.sleep(forTimeInterval: arguments["fixtureDelay"] as? Double ?? 0)
            mutations += 1
            return (["mutations": mutations], Data())
        }
        throw Failure(code: "CAPABILITY_UNAVAILABLE", message: "Fixture operation unavailable")
    }
}

@main
struct Harness {
    @MainActor static func main() async throws {
        let runner = TestrunRunnerTests()
        let configuration = TestrunRunnerTests.Configuration(
            sessionId: "Acme", token: String(repeating: "a", count: 64),
            expiresAt: Date(timeIntervalSinceNow: 120))
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        let listener = try NWListener(using: parameters)
        let network = DispatchQueue(label: "Acme.runner.network")
        listener.newConnectionHandler = { connection in
            connection.start(queue: network)
            runner.receiveRequest(connection, configuration: configuration)
        }
        listener.stateUpdateHandler = { state in
            if case .ready = state {
                print("{\"port\":\(listener.port!.rawValue)}")
                fflush(stdout)
            }
        }
        listener.start(queue: network)
        while !runner.stopped { try await Task.sleep(nanoseconds: 10_000_000) }
        listener.cancel()
    }
}
