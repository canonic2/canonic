// Owns screenshots, bounded accessibility translation and session-scoped element snapshots.
import Foundation
import UIKit
@preconcurrency import XCTest

extension TestrunRunnerTests {
    private func role(_ type: XCUIElement.ElementType) -> String {
        switch type {
        case .application: return "application"
        case .button: return "button"
        case .staticText: return "text"
        case .textField, .searchField: return "text_field"
        case .secureTextField: return "secure_text_field"
        case .textView: return "text_area"
        case .switch: return "switch"
        case .slider: return "slider"
        case .alert: return "alert"
        // Tables and collections scroll and build only rows near the screen.
        case .scrollView, .table, .collectionView: return "scroll_view"
        case .image: return "image"
        default: return "element"
        }
    }
    private func tree(
        _ snapshot: XCUIElementSnapshot, element: XCUIElement, depth: Int, count: inout Int,
        references: inout [String: XCUIElement], truncated: inout Bool
    ) throws -> [String: Any]? {
        if let currentRequest { try requests.check(currentRequest) }
        // XCTest snapshots stop at 60 levels and fold anything deeper into the
        // last one without saying so, so a node that reaches that level may
        // hide descendants and makes the tree partial.
        guard depth < 59, count < 500 else {
            truncated = true
            return nil
        }
        count += 1
        let ref = "node-\(count)"
        references[ref] = element
        let frame = snapshot.frame
        let secure = snapshot.elementType == .secureTextField
        var node: [String: Any] = [
            "elementRef": ref, "role": role(snapshot.elementType), "identifier": snapshot.identifier,
            "label": String(snapshot.label.prefix(8192)), "secure": secure,
            "frame": [
                "x": frame.minX, "y": frame.minY, "width": frame.width, "height": frame.height,
            ],
            "states": ["enabled": snapshot.isEnabled, "selected": snapshot.isSelected],
        ]
        if !secure, let value = snapshot.value as? String {
            node["value"] = String(value.prefix(8192))
        }
        var children: [[String: Any]] = []
        for (index, child) in snapshot.children.enumerated() {
            // Query construction is lazy. Resolve/hit-test only when an explicit
            // element action is admitted, not once per attribute during a read.
            if let node = try tree(
                child, element: element.children(matching: .any).element(boundBy: index),
                depth: depth + 1, count: &count, references: &references,
                truncated: &truncated)
            {
                children.append(node)
            }
            if count >= 500 {
                truncated = true
                break
            }
        }
        node["children"] = children
        return node
    }
    func observe(_ action: String, arguments: [String: Any]) throws -> ([String: Any], Data) {
        switch action {
        case "screenshot":
            if arguments["profile"] as? String == "fast" { return try fastScreenshot() }
            let image = XCUIScreen.main.screenshot().image
            let format = arguments["format"] as? String ?? "png"
            guard format == "png" || format == "jpeg",
                let bytes = format == "png"
                    ? image.pngData() : image.jpegData(compressionQuality: 0.75)
            else {
                throw Failure(
                    code: "CAPTURE_FAILED", message: "Device screenshot could not be encoded")
            }
            return (
                [
                    "width": image.cgImage?.width ?? 0, "height": image.cgImage?.height ?? 0,
                    "mimeType": format == "png" ? "image/png" : "image/jpeg",
                    "sourceMode": "device_frame_sequence",
                ], bytes
            )
        case "ui_snapshot":
            let app = try scope(arguments)
            let captured = try app.snapshot()
            if let currentRequest { try requests.check(currentRequest) }
            var count = 0
            var references: [String: XCUIElement] = [:]
            var truncated = false
            let root =
                try tree(captured, element: app, depth: 0, count: &count,
                    references: &references, truncated: &truncated)
                ?? [:]
            snapshots = snapshots.filter { Date().timeIntervalSince($0.value.0) <= 30 }
            if snapshots.count >= 8,
                let oldest = snapshots.min(by: { $0.value.0 < $1.value.0 })?.key
            {
                snapshots.removeValue(forKey: oldest)
            }
            let scopeKey = try JSONSerialization.data(
                withJSONObject: arguments["scope"] ?? [:], options: .sortedKeys)
            let id = UUID().uuidString
            snapshots[id] = (Date(), scopeKey, references)
            // Point input normalizes against this frame, so frames convert with it.
            let display = XCUIApplication(bundleIdentifier: "com.apple.springboard").frame
            return (
                [
                    "snapshotId": id, "quality": truncated ? "partial" : "complete",
                    "nodeCount": count, "truncated": truncated, "root": root,
                    "display": [
                        "x": display.minX, "y": display.minY, "width": display.width,
                        "height": display.height,
                    ],
                    "unavailableAttributes": ["hittable"],
                ], Data()
            )
        default:
            throw Failure(code: "CAPABILITY_UNAVAILABLE", message: "Observation is not supported")
        }
    }
    private func fastScreenshot() throws -> ([String: Any], Data) {
        let start = ProcessInfo.processInfo.systemUptime
        let original = XCUIScreen.main.screenshot().image
        guard let pixels = original.cgImage else {
            throw Failure(code: "CAPTURE_FAILED", message: "Screenshot has no pixels")
        }
        let acquired = ProcessInfo.processInfo.systemUptime
        let factor = min(1.0, 960.0 / Double(max(pixels.width, pixels.height)))
        let size = CGSize(
            width: max(1, floor(Double(pixels.width) * factor)),
            height: max(1, floor(Double(pixels.height) * factor)))
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.opaque = true
        let image = UIGraphicsImageRenderer(size: size, format: format).image { _ in
            original.draw(in: CGRect(origin: .zero, size: size))
        }
        guard let bytes = image.jpegData(compressionQuality: 0.55) else {
            throw Failure(code: "CAPTURE_FAILED", message: "Fast screenshot encoding failed")
        }
        return (
            [
                "width": Int(size.width), "height": Int(size.height), "sourceWidth": pixels.width,
                "sourceHeight": pixels.height,
                "mimeType": "image/jpeg", "sourceMode": "device_frame_sequence",
                "timingsMs": [
                    "acquisition": (acquired - start) * 1000,
                    "encoding": (ProcessInfo.processInfo.systemUptime - acquired) * 1000,
                ],
            ], bytes
        )
    }
}
