import UIKit
import os

@main @MainActor final class AcmeApp: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = AcmeScreen(); window.makeKeyAndVisible(); self.window = window
        return true
    }
}
@MainActor final class AcmeScreen: UIViewController {
    let corrected = UISwitch(), armed = UISwitch(), status = UILabel()
    private let log = Logger(subsystem: "com.example.acme.diagnostics", category: "fixture")
    override func viewDidLoad() {
        super.viewDidLoad(); view.backgroundColor = .systemBackground
        let launches = UserDefaults.standard.integer(forKey: "launches") + 1
        UserDefaults.standard.set(launches, forKey: "launches")
        let scroll = UIScrollView(); scroll.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(scroll)
        let stack = UIStackView(); stack.axis = .vertical; stack.alignment = .leading; stack.spacing = 12; stack.translatesAutoresizingMaskIntoConstraints = false; scroll.addSubview(stack)
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 20), scroll.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -20),
            scroll.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor), scroll.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor), stack.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor),
            stack.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor), stack.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor), stack.widthAnchor.constraint(equalTo: scroll.frameLayoutGuide.widthAnchor)
        ])
        func label(_ text: String) { let label = UILabel(); label.text = text; label.numberOfLines = 0; stack.addArrangedSubview(label) }
        label("Acme native iOS\nLaunch \(launches)\nBirth \(UUID().uuidString)")
        corrected.accessibilityIdentifier = "acme-corrected"; corrected.accessibilityLabel = "Corrected mode"
        armed.accessibilityIdentifier = "acme-armed"; armed.accessibilityLabel = "Arm app crash / hang"
        label("Corrected mode"); stack.addArrangedSubview(corrected)
        label("Arm app crash / hang"); stack.addArrangedSubview(armed)
        status.text = "Acme ready"; status.numberOfLines = 0; status.accessibilityIdentifier = "acme-result"; stack.addArrangedSubview(status)
        func button(_ title: String, fatal: Bool = false, _ action: @escaping @MainActor () -> String) {
            let button = UIButton(type: .system); button.setTitle(title, for: .normal); button.accessibilityIdentifier = title
            button.addAction(UIAction { [weak self] _ in
                guard let self else { return }
                if fatal && !self.armed.isOn { self.status.text = "Arm this app first"; return }
                if fatal { self.armed.isOn = false }
                FileHandle.standardError.write(Data("Acme trigger \(title) corrected=\(self.corrected.isOn)\n".utf8))
                self.log.notice("Acme action \(title, privacy: .public) corrected=\(self.corrected.isOn)")
                self.status.text = action()
                self.log.notice("Acme result \(self.status.text ?? "", privacy: .public)")
            }, for: .touchUpInside); stack.addArrangedSubview(button)
        }
        button("Retain one MiB") { [unowned self] in String(AcmeSwiftFaults.retain(corrected: corrected.isOn)) }
        button("Native retained bytes") { [unowned self] in String(acme_native_retain(corrected.isOn)) }
        button("Reset") { [unowned self] in AcmeSwiftFaults.reset(); _ = acme_native_retain(true); armed.isOn = false; return "Acme reset" }
        button("Swift crash", fatal: true) { [unowned self] in AcmeSwiftFaults.crash(corrected: corrected.isOn); return "ready" }
        button("Objective-C exception", fatal: true) { [unowned self] in AcmeObjCFaults.exceptionCorrected(corrected.isOn); return "ready" }
        button("C++ crash", fatal: true) { [unowned self] in acme_native_crash(corrected.isOn); return "ready" }
        button("Main-thread hang (6 seconds)", fatal: true) { [unowned self] in AcmeSwiftFaults.mainThreadHang(corrected: corrected.isOn); return "hang requested" }
        button("Background crash", fatal: true) { [unowned self] in AcmeSwiftFaults.backgroundFailure(corrected: corrected.isOn); return "background requested" }
        log.notice("Acme ready launch=\(launches)")
    }
    deinit { _ = acme_native_retain(true) }
}
