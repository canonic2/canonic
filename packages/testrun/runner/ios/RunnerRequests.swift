// Owns admission outside XCTest's UI executor; expired reads never queue later input.
import Foundation

final class RunnerRequests: @unchecked Sendable {
    struct Ticket: Sendable {
        let id: UUID
        let inspection: Bool
        let deadline: Double
        let action: String
        let started: Double
    }
    enum Refusal: String, Error {
        case busy = "RUNNER_BUSY"
        case closed = "SESSION_CLOSED"
        case expired = "INSPECTION_TIMEOUT"
    }
    enum Closing {
        case idle, inspection
        case input(Ticket)
    }
    enum InputProgress { case finished, running, overdue }
    private let lock = NSLock()
    private var active: Ticket?
    private var closed = false
    private var activity = ProcessInfo.processInfo.systemUptime
    /// Longest admitted input, including a 10-second gesture and XCTest overhead.
    /// Input still running past it at stop is treated as wedged.
    let inputGrace: Double
    init(inputGrace: Double = 15) {
        self.inputGrace = inputGrace
    }
    func touch() {
        lock.lock()
        defer { lock.unlock() }
        activity = ProcessInfo.processInfo.systemUptime
    }
    var idleExpired: Bool {
        lock.lock()
        defer { lock.unlock() }
        return ProcessInfo.processInfo.systemUptime - activity >= 30
    }
    func begin(action: String) throws -> Ticket {
        lock.lock()
        defer { lock.unlock() }
        guard !closed else { throw Refusal.closed }
        guard active == nil else { throw Refusal.busy }
        let now = ProcessInfo.processInfo.systemUptime
        let ticket = Ticket(id: UUID(), inspection: action == "ui_snapshot",
            deadline: now + 4, action: action, started: now)
        active = ticket
        return ticket
    }
    func check(_ ticket: Ticket) throws {
        lock.lock()
        defer { lock.unlock() }
        guard !closed, active?.id == ticket.id else { throw Refusal.closed }
        if ticket.inspection && ProcessInfo.processInfo.systemUptime >= ticket.deadline {
            throw Refusal.expired
        }
    }
    func finish(_ ticket: Ticket) {
        lock.lock()
        defer { lock.unlock() }
        if active?.id == ticket.id { active = nil }
    }
    /// Closing refuses new work at once. An inspection may be aborted with process
    /// exit; active input gets until its grace expires to finish first.
    func close() -> Closing {
        lock.lock()
        defer { lock.unlock() }
        closed = true
        guard let active else { return .idle }
        return active.inspection ? .inspection : .input(active)
    }
    func progress(_ ticket: Ticket) -> InputProgress {
        lock.lock()
        defer { lock.unlock() }
        guard active?.id == ticket.id else { return .finished }
        return ProcessInfo.processInfo.systemUptime - ticket.started >= inputGrace
            ? .overdue : .running
    }
    var health: [String: Any] {
        lock.lock()
        defer { lock.unlock() }
        return [
            "state": closed ? "closed" : active == nil ? "ready" : "busy",
            "inspectionExpired": active.map {
                $0.inspection && ProcessInfo.processInfo.systemUptime >= $0.deadline
            } ?? false,
            "activeAction": active.map { $0.action as Any } ?? NSNull(),
            "activeElapsedMs": active.map {
                max(0, ProcessInfo.processInfo.systemUptime - $0.started) * 1000
            } ?? 0,
        ]
    }
}
