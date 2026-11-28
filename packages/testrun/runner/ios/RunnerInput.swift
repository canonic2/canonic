// Owns complete scoped gestures, focus-proven typing and input admission.
import Foundation
import UIKit
@preconcurrency import XCTest

extension TestrunRunnerTests {
    // Conservative checks for point input: no automatic interruption dismissal.
    private func admitTap(
        _ app: XCUIApplication, frame: CGRect, display: CGRect, point: CGPoint,
        inspectVisibility: Bool = true
    ) throws {
        guard app.state == .runningForeground else {
            throw Failure(code: "APP_BACKGROUNDED", message: "Scoped app is not foreground")
        }
        guard !stopped, Date() < configuration.expiresAt else {
            throw Failure(code: "SESSION_CLOSED", message: "Runner session ended before dispatch")
        }
        if !inspectVisibility {
            guard app.frame == frame else {
                throw Failure(code: "UI_TRANSITION", message: "Application geometry is changing")
            }
            return
        }
        var error: NSError?
        guard SBCheckTapVisibility(app, frame, point, &error) else {
            let code =
                error?.code == 3
                ? "UI_OCCLUDED" : error?.code == 4 ? "UI_TRANSITION" : "CAPABILITY_UNAVAILABLE"
            throw Failure(
                code: code, message: error?.localizedDescription ?? "Visibility inspection failed")
        }
        guard app.state == .runningForeground else {
            throw Failure(
                code: "UI_TRANSITION", message: "Application geometry or visibility is changing")
        }
    }
    private func position(frame: CGRect, display: CGRect, point: [String: Any]) throws -> CGPoint {
        guard let x = point["x"] as? Double, let y = point["y"] as? Double,
            x.isFinite, y.isFinite, (0...1).contains(x), (0...1).contains(y)
        else {
            throw Failure(
                code: "INVALID_ARGUMENT", message: "Point must be normalized within the display")
        }
        let position = CGPoint(
            x: display.minX + display.width * x, y: display.minY + display.height * y)
        guard frame.contains(position) else {
            throw Failure(code: "OUT_OF_SCOPE", message: "Point is outside the scoped application")
        }
        return position
    }
    private func coordinate(
        _ app: XCUIApplication, frame: CGRect, display: CGRect, point: [String: Any]
    ) throws -> XCUICoordinate {
        let position = try position(frame: frame, display: display, point: point)
        return app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: position.x - frame.minX, dy: position.y - frame.minY))
    }
    func performGesture(
        _ action: String, arguments: [String: Any], admitted: Double, geometryReady: Double
    ) throws -> ([String: Any], Data) {
        let issueCount = inputIssues.count
        let app = try scope(arguments, requireForeground: true)
        let frame = app.frame
        let display = XCUIApplication(bundleIdentifier: "com.apple.springboard").frame
        let scopeReady = ProcessInfo.processInfo.systemUptime
        guard let point = arguments[action == "swipe" ? "from" : "point"] as? [String: Any] else {
            throw Failure(code: "INVALID_ARGUMENT", message: "A typed point is required")
        }
        let position = try position(frame: frame, display: display, point: point)
        let coordinateReady = ProcessInfo.processInfo.systemUptime
        var adapter: [String: Any] = [:]
        if action == "tap", (arguments["scope"] as? [String: Any])?["kind"] as? String == "app" {
            try admitTap(
                app, frame: frame, display: display, point: position, inspectVisibility: false)
            RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.05))
            var admissionFailure: Failure?
            var nativeError: NSError?
            do {
                let synthesis = SBConcreteTap(
                    position, app, 5,
                    { error in
                        do {
                            try self.admitTap(app, frame: frame, display: display, point: position)
                            guard self.inputIssues.count == issueCount else {
                                throw Failure(
                                    code: "OUTCOME_UNKNOWN", message: "XCTest recorded an issue")
                            }
                            return true
                        } catch let failure as Failure {
                            admissionFailure = failure
                            error?.pointee = NSError(domain: "TestrunTapAdmission", code: 3)
                            return false
                        } catch { return false }
                    }, &nativeError)
                guard let synthesis else {
                    throw nativeError
                        ?? NSError(
                            domain: "TestrunConcreteTap", code: 2,
                            userInfo: [
                                NSLocalizedDescriptionKey: "Concrete event returned no completion"
                            ])
                }
                adapter = synthesis as? [String: Any] ?? [:]
            } catch {
                if let failure = admissionFailure { throw failure }
                let native = error as NSError
                throw Failure(
                    code: native.code == 2
                        ? "OUTCOME_UNKNOWN"
                        : native.code == 3 ? "UI_TRANSITION" : "CAPABILITY_UNAVAILABLE",
                    message: native.localizedDescription)
            }
            adapter["adapter"] = "guarded_concrete_event"
        } else if action == "tap" {
            try coordinate(app, frame: frame, display: display, point: point).tap()
        } else {
            let start = try coordinate(app, frame: frame, display: display, point: point)
            guard let duration = arguments["durationMs"] as? Double,
                (0...10_000).contains(duration), duration.rounded() == duration
            else {
                throw Failure(
                    code: "INVALID_ARGUMENT",
                    message: "durationMs must be an integer between 0 and 10000 ms")
            }
            if action == "long_press" {
                start.press(forDuration: duration / 1000)
            } else {
                guard let end = arguments["to"] as? [String: Any] else {
                    throw Failure(code: "INVALID_ARGUMENT", message: "Swipe requires an end point")
                }
                let destination = try coordinate(app, frame: frame, display: display, point: end)
                let delta = CGPoint(
                    x: destination.screenPoint.x - start.screenPoint.x,
                    y: destination.screenPoint.y - start.screenPoint.y)
                let velocity =
                    duration == 0
                    ? XCUIGestureVelocity.fast
                    : XCUIGestureVelocity(
                        rawValue: max(1, hypot(delta.x, delta.y) / (duration / 1000)))
                start.press(
                    forDuration: duration == 0 ? 0 : 0.01, thenDragTo: destination,
                    withVelocity: velocity, thenHoldForDuration: 0)
            }
        }
        return (
            [
                "completed": true, "observed": false, "nativeInput": adapter,
                "timingsMs": [
                    "geometry": (geometryReady - admitted) * 1000,
                    "scope": (scopeReady - geometryReady) * 1000,
                    "coordinate": (coordinateReady - scopeReady) * 1000,
                    "execution": (ProcessInfo.processInfo.systemUptime - coordinateReady) * 1000,
                    "input": (ProcessInfo.processInfo.systemUptime - scopeReady) * 1000,
                ],
            ], Data()
        )
    }
    func typeText(_ arguments: [String: Any]) throws -> ([String: Any], Data) {
        let issueCount = inputIssues.count
        guard arguments["replace"] as? Bool != true else {
            throw Failure(
                code: "CAPABILITY_UNAVAILABLE",
                message: "Device replacement typing requires a verified semantic editing adapter")
        }
        let app = try scope(arguments, requireForeground: true)
        if arguments["snapshotId"] == nil && arguments["elementRef"] == nil {
            var error: NSError?
            guard
                let element = SBFocusedEditableInApplication(app, &error)
            else {
                throw Failure(
                    code: error?.code == 1 ? "CAPABILITY_UNAVAILABLE" : "FOCUS_NOT_PROVEN",
                    message: error?.localizedDescription ?? "Focus an editable field before typing")
            }
            guard inputIssues.count == issueCount else {
                throw Failure(
                    code: "OUTCOME_UNKNOWN",
                    message: "XCTest recorded an issue while resolving focus")
            }
            guard app.state == .runningForeground else {
                throw Failure(code: "APP_BACKGROUNDED", message: "Scoped app is not foreground")
            }
            element.typeText(try string(arguments, "text"))
            return (["completed": true, "observed": false], Data())
        }
        let scopeKey = try JSONSerialization.data(
            withJSONObject: arguments["scope"] ?? [:], options: .sortedKeys)
        let snapshotId = try string(arguments, "snapshotId")
        let ref = try string(arguments, "elementRef")
        guard let snapshot = snapshots[snapshotId], Date().timeIntervalSince(snapshot.0) <= 30,
            snapshot.1 == scopeKey, let element = snapshot.2[ref], element.exists,
            element.isHittable,
            [.textField, .secureTextField, .textView, .searchField].contains(element.elementType)
        else {
            throw Failure(
                code: "STALE_ELEMENT", message: "A fresh hittable editable element is required")
        }
        element.tap()
        element.typeText(try string(arguments, "text"))
        return (["completed": true, "observed": false], Data())
    }
}
