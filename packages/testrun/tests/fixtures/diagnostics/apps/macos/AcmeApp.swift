import AppKit

@main @MainActor enum AcmeMain {
    static func main() {
        let app = NSApplication.shared
        let owner = AcmeApp(); app.delegate = owner; app.setActivationPolicy(.regular)
        withExtendedLifetime(owner) { app.run() }
    }
}
@MainActor final class AcmeApp: NSObject, NSApplicationDelegate {
    var window: NSWindow?
    let corrected = NSButton(checkboxWithTitle: "Corrected mode", target: nil, action: nil)
    let armed = NSButton(checkboxWithTitle: "Arm app crash / 6-second hang", target: nil, action: nil)
    let status = NSTextField(labelWithString: "Acme ready")
    var buttons: [Int: NSButton] = [:]
    var driverCommands = 0
    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 460, height: 520), styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "Acme Diagnostics"; self.window = window
        let stack = NSStackView(); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 12; stack.translatesAutoresizingMaskIntoConstraints = false
        window.contentView!.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: window.contentView!.leadingAnchor, constant: 20), stack.topAnchor.constraint(equalTo: window.contentView!.topAnchor, constant: 20), stack.trailingAnchor.constraint(equalTo: window.contentView!.trailingAnchor, constant: -20)])
        stack.addArrangedSubview(NSTextField(labelWithString: "Acme native macOS diagnostics")); stack.addArrangedSubview(corrected); stack.addArrangedSubview(armed); stack.addArrangedSubview(status)
        for (id, title) in [(11,"Bounded CPU work"),(12,"Retain one MiB"),(13,"Reset"),(20,"Swift crash"),(21,"Objective-C background exception"),(22,"C++ crash"),(23,"UI-thread hang (6 seconds)"),(24,"Background crash")] {
            let button = NSButton(title: title, target: self, action: #selector(run)); button.tag = id; buttons[id] = button; stack.addArrangedSubview(button)
        }
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        if ProcessInfo.processInfo.arguments.contains("--acme-driver") {
            emit("ready", nanoseconds: 0)
            Thread.detachNewThread { [weak self] in
                var line = Data()
                while let byte = try? FileHandle.standardInput.read(upToCount: 1), !byte.isEmpty {
                    if byte[0] == 10 {
                        let command = String(data: line, encoding: .utf8) ?? "invalid"; line.removeAll(keepingCapacity: true)
                        DispatchQueue.main.async { self?.drive(command) }
                    } else {
                        line.append(byte[0]); if line.count > 64 { DispatchQueue.main.async { NSApp.terminate(nil) }; return }
                    }
                }
                DispatchQueue.main.async { NSApp.terminate(nil) }
            }
        }
    }
    func emit(_ command: String, nanoseconds: UInt64) {
        let value: [String: Any] = ["command":command,"pid":ProcessInfo.processInfo.processIdentifier,"status":status.stringValue,"corrected":corrected.state == .on,"armed":armed.state == .on,"retainedBytes":AcmeSwiftFaults.retainedBytes,"actionNanoseconds":nanoseconds]
        if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) { FileHandle.standardOutput.write(data); FileHandle.standardOutput.write(Data([10])) }
    }
    func drive(_ command: String) {
        driverCommands += 1
        guard driverCommands <= 64 else { NSApp.terminate(nil); return }
        let before = DispatchTime.now().uptimeNanoseconds
        switch command {
        case "corrected-on": corrected.state = .on
        case "corrected-off": corrected.state = .off
        case "arm": armed.state = .on
        case "quit": emit(command, nanoseconds: 0); NSApp.terminate(nil); return
        default:
            let commands = ["cpu":11,"retain":12,"reset":13,"swift-crash":20,"objc-crash":21,"cpp-crash":22,"hang":23,"background-crash":24]
            guard let tag = commands[command], let button = buttons[tag] else { status.stringValue = "Unknown Acme command"; emit(command, nanoseconds: 0); return }
            button.performClick(nil)
        }
        emit(command, nanoseconds: DispatchTime.now().uptimeNanoseconds - before)
    }
    @objc func run(_ sender: NSButton) {
        let control = corrected.state == .on
        if sender.tag >= 20 { guard armed.state == .on else { status.stringValue = "Arm this app first"; return }; armed.state = .off }
        switch sender.tag {
        case 11: var checksum: UInt32 = 0; for value in 0..<(control ? 100 : 100000) { checksum &+= UInt32(value) }; status.stringValue = String(checksum)
        case 12: status.stringValue = String(AcmeSwiftFaults.retain(corrected: control))
        case 13: AcmeSwiftFaults.reset(); _ = acme_native_retain(true); armed.state = .off; status.stringValue = "Acme reset"
        case 20: AcmeSwiftFaults.crash(corrected: control)
        case 21: AcmeObjCFaults.backgroundFailureCorrected(control)
        case 22: acme_native_crash(control)
        case 23: AcmeSwiftFaults.mainThreadHang(corrected: control)
        case 24: AcmeSwiftFaults.backgroundFailure(corrected: control)
        default: break
        }
        if sender.tag >= 20 { status.stringValue = control ? "Corrected control ready" : "Fault requested" }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ notification: Notification) { AcmeSwiftFaults.reset(); _ = acme_native_retain(true) }
}
