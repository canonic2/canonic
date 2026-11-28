import CryptoKit
// Owns authenticated runner framing, request admission, deduplication and replies.
import Foundation
import Darwin
@preconcurrency import Network
@preconcurrency import XCTest

extension TestrunRunnerTests {
    nonisolated private static func receiveExactly(
        _ connection: NWConnection, count: Int,
        accumulated: Data = Data(), completion: @escaping (Data?) -> Void
    ) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: count - accumulated.count) {
            data, _, ended, error in
            guard error == nil, let data, !data.isEmpty else {
                completion(nil)
                return
            }
            var bytes = accumulated
            bytes.append(data)
            if bytes.count == count {
                completion(bytes)
            } else if ended {
                completion(nil)
            } else {
                Self.receiveExactly(
                    connection, count: count, accumulated: bytes, completion: completion)
            }
        }
    }
    nonisolated func receiveRequest(_ connection: NWConnection, configuration: Configuration) {
        Self.receiveExactly(connection, count: 4) { header in
            guard let header else {
                connection.cancel()
                return
            }
            let length = header.reduce(0) { ($0 << 8) | Int($1) }
            guard (1...1_048_576).contains(length) else {
                connection.cancel()
                return
            }
            Self.receiveExactly(connection, count: length) { data in
                guard let data else {
                    connection.cancel()
                    return
                }
                self.admit(connection, data: data, configuration: configuration)
            }
        }
    }
    // Authentication, heartbeat and admission must not wait behind a synchronous XCTest read.
    nonisolated private func admit(
        _ connection: NWConnection, data: Data, configuration: Configuration
    ) {
        var requestId = ""
        do {
            guard let request = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                request["version"] as? Int == 1,
                request["sessionId"] as? String == configuration.sessionId,
                let token = request["token"] as? String, constantEqual(token, configuration.token),
                Date() < configuration.expiresAt, let id = request["requestId"] as? String,
                let action = request["action"] as? String,
                request["arguments"] is [String: Any]
            else {
                send(connection, response: ["ok": false, "code": "UNAUTHORIZED",
                    "message": "Runner session authentication failed"], configuration: configuration)
                return
            }
            requestId = id
            requests.touch()
            if action == "hello" {
                send(connection, response: ["requestId": id, "ok": true, "data": [
                    "protocolVersion": 1, "runnerPid": getpid(), "sourceMode": "device_frame_sequence", "fpsCeiling": 5,
                    "capabilities": ["appManagement", "tap", "longPress", "swipe", "text",
                        "accessibility", "screenshots"], "inspection": requests.health,
                ]], configuration: configuration)
                return
            }
            if action == "stop" {
                switch requests.close() {
                case .idle:
                    replyStopped(connection, id: id, configuration: configuration)
                case .inspection:
                    // XCTest has no cancellation API for an in-progress snapshot.
                    replyStopped(
                        connection, id: id, configuration: configuration, aborting: "ui_snapshot")
                case .input(let ticket):
                    stopAfter(ticket, connection: connection, id: id, configuration: configuration)
                }
                return
            }
            let ticket = try requests.begin(action: action)
            Task { @MainActor in
                self.handle(connection, data: data, ticket: ticket,
                    configuration: configuration, responseId: id)
            }
        } catch let refusal as RunnerRequests.Refusal {
            send(connection, response: ["requestId": requestId, "ok": false,
                "code": refusal.rawValue, "message": "Runner is busy or closing; no work was queued",
                "retryable": refusal == .busy], configuration: configuration)
        } catch {
            connection.cancel()
        }
    }
    /// The host stops a runner only after abandoning any reply it awaited, so active
    /// input already has an unknown, never replayed outcome. A gesture that finishes
    /// within its grace ends normally; one still running is wedged in XCTest, which
    /// cannot cancel it, and only exiting this owned runner releases the session.
    nonisolated private func stopAfter(
        _ ticket: RunnerRequests.Ticket, connection: NWConnection, id: String,
        configuration: Configuration
    ) {
        switch requests.progress(ticket) {
        case .finished:
            replyStopped(connection, id: id, configuration: configuration)
        case .overdue:
            replyStopped(connection, id: id, configuration: configuration, aborting: ticket.action)
        case .running:
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.05) {
                self.stopAfter(ticket, connection: connection, id: id, configuration: configuration)
            }
        }
    }
    nonisolated private func replyStopped(
        _ connection: NWConnection, id: String, configuration: Configuration,
        aborting action: String? = nil
    ) {
        send(connection, response: ["requestId": id, "ok": true,
            "data": ["stopped": true, "abortedAction": action.map { $0 as Any } ?? NSNull()]],
            configuration: configuration, terminal: true) {
            if action != nil { exit(EXIT_SUCCESS) }
            Task { @MainActor in self.stopped = true }
        }
    }
    private func handle(
        _ connection: NWConnection, data: Data, ticket: RunnerRequests.Ticket,
        configuration: Configuration, responseId: String
    ) {
        let started = ProcessInfo.processInfo.systemUptime
        var activeCommand: String?
        let issueCount = inputIssues.count
        let observing = ticket.inspection && inputIssues.beginObservation()
        defer {
            if observing { inputIssues.finishObservation() }
            currentRequest = nil
            requests.finish(ticket)
            if activeCommand != nil { _ = inputIssues.finishCommand(withUncertainOutcome: false) }
        }
        do {
            try requests.check(ticket)
            if ticket.inspection && !observing {
                throw Failure(code: "RUNNER_BUSY", message: "Runner input is in progress")
            }
            currentRequest = ticket
            guard let request = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                request["version"] as? Int == 1,
                request["sessionId"] as? String == configuration.sessionId,
                let token = request["token"] as? String, constantEqual(token, configuration.token),
                Date() < configuration.expiresAt, let requestId = request["requestId"] as? String,
                let action = request["action"] as? String,
                let arguments = request["arguments"] as? [String: Any]
            else {
                throw Failure(code: "UNAUTHORIZED", message: "Runner session authentication failed")
            }
            let mutations: Set<String> = [
                "tap", "long_press", "swipe", "type_text", "app_launch", "app_terminate",
            ]
            var commandId: String?
            if mutations.contains(action) {
                guard let id = request["commandId"] as? String, (1...128).contains(id.utf8.count)
                else {
                    throw Failure(
                        code: "INVALID_ARGUMENT", message: "A bounded command ID is required")
                }
                commandId = id
                let fingerprint = Data(
                    SHA256.hash(
                        data: try JSONSerialization.data(
                            withJSONObject: ["action": action, "arguments": arguments],
                            options: .sortedKeys)))
                if let previous = commands[id] {
                    guard previous.0 == fingerprint else {
                        throw Failure(
                            code: "COMMAND_ID_CONFLICT",
                            message: "Command ID has different arguments")
                    }
                    send(
                        connection,
                        response: ["requestId": requestId, "ok": true, "data": previous.1],
                        configuration: configuration)
                    return
                }
                guard commands.count < 10000 else {
                    throw Failure(code: "RESOURCE_LIMIT", message: "Runner command journal is full")
                }
                let admission = inputIssues.beginCommand()
                guard admission != 1 else {
                    throw Failure(code: "TARGET_BUSY", message: "Runner input is in progress")
                }
                guard admission == 0 else {
                    throw Failure(
                        code: "OUTCOME_UNKNOWN",
                        message: "Runner input is uncertain; close and reopen the session")
                }
                commands[id] = (fingerprint, ["commandId": id, "state": "dispatched"])
                activeCommand = id
            }
            let (result, payload) = try execute(action, arguments: arguments)
            // Input that finished while stop waited still reports its real outcome.
            if ticket.inspection { try requests.check(ticket) }
            guard inputIssues.count == issueCount else {
                throw Failure(
                    code: ticket.inspection ? "INSPECTION_FAILED" : "OUTCOME_UNKNOWN",
                    message: "XCTest recorded an issue during the request")
            }
            var data = result
            if let commandId {
                data = [
                    "commandId": commandId, "state": "completed", "observed": false,
                    "result": result,
                ]
                commands[commandId]!.1 = data
            }
            send(
                connection,
                response: [
                    "requestId": requestId, "ok": true, "data": data,
                    "runnerElapsedMs": (ProcessInfo.processInfo.systemUptime - started) * 1000,
                ], configuration: configuration, payload: payload)
        } catch let refusal as RunnerRequests.Refusal {
            send(connection, response: ["requestId": responseId, "ok": false,
                "code": refusal.rawValue,
                "message": "Inspection expired or session closed; no snapshot was published",
                "retryable": false], configuration: configuration)
        } catch let failure as Failure {
            let unknown = failure.code == "OUTCOME_UNKNOWN"
                || !ticket.inspection && inputIssues.count != issueCount
            let code = unknown ? "OUTCOME_UNKNOWN" : failure.code
            let message =
                unknown ? "Runner input outcome is uncertain; do not replay" : failure.message
            if let id = activeCommand, commands[id] != nil {
                _ = inputIssues.finishCommand(withUncertainOutcome: unknown)
                commands[id]!.1 = [
                    "commandId": id, "state": unknown ? "outcome_unknown" : "failed",
                    "error": ["code": code, "message": message],
                ]
            }
            send(
                connection,
                response: [
                    "requestId": responseId, "ok": false, "code": code, "message": message,
                    "retryable": false,
                ], configuration: configuration)
        } catch {
            if activeCommand != nil { _ = inputIssues.finishCommand(withUncertainOutcome: true) }
            if let id = activeCommand, commands[id] != nil {
                commands[id]!.1 = [
                    "commandId": id, "state": "outcome_unknown",
                    "error": [
                        "code": "OUTCOME_UNKNOWN",
                        "message": "Owned runner operation failed after admission",
                    ],
                ]
            }
            send(
                connection,
                response: [
                    "requestId": responseId, "ok": false,
                    "code": ticket.inspection ? "INSPECTION_FAILED" : "OUTCOME_UNKNOWN",
                    "message": "Owned runner operation failed", "retryable": false,
                ], configuration: configuration)
        }
    }
    nonisolated private func constantEqual(_ a: String, _ b: String) -> Bool {
        let left = Array(a.utf8)
        let right = Array(b.utf8)
        guard left.count == right.count else { return false }
        return zip(left, right).reduce(UInt8(0)) { $0 | ($1.0 ^ $1.1) } == 0
    }
    nonisolated private func send(
        _ connection: NWConnection, response: [String: Any], configuration: Configuration,
        payload: Data = Data(), terminal: Bool = false,
        completion: (@Sendable () -> Void)? = nil
    ) {
        var response = response
        response["payloadLength"] = payload.count
        guard let bytes = try? JSONSerialization.data(withJSONObject: response),
            bytes.count <= 2_097_152,
            payload.count <= 67_108_864
        else {
            connection.cancel()
            return
        }
        var length = UInt32(bytes.count).bigEndian
        var packet = Data(bytes: &length, count: 4)
        packet.append(bytes)
        packet.append(payload)
        connection.send(
            content: packet,
            completion: .contentProcessed { error in
                if error != nil || terminal {
                    connection.cancel()
                } else {
                    self.receiveRequest(connection, configuration: configuration)
                }
                completion?()
            })
    }
}
