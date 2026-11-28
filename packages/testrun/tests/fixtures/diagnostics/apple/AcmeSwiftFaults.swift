import Foundation

@MainActor enum AcmeSwiftFaults {
    private static var retained: [Data] = []
    static var retainedBytes: Int { retained.reduce(0) { $0 + $1.count } }
    static func crash(corrected: Bool) { if !corrected { fatalError("Acme Swift fixture failure") } }
    static func mainThreadHang(corrected: Bool) { if !corrected { DispatchQueue.main.async { Thread.sleep(forTimeInterval: 6) } } }
    static func backgroundFailure(corrected: Bool) { if !corrected { DispatchQueue.global().async { fatalError("Acme Swift background failure") } } }
    static func retain(corrected: Bool) -> Int { retained = corrected ? [] : (0..<16).map { _ in Data(repeating: 1, count: 65536) }; return retained.reduce(0) { $0 + $1.count } }
    static func reset() { retained = [] }
}
