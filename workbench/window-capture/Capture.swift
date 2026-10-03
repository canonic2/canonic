import AppKit
import CoreGraphics
import CoreMedia
import CoreVideo
import Dispatch
import Foundation
import ScreenCaptureKit
import VideoToolbox

/* Streams one on-screen window, chosen by its application and title, as
   length-prefixed H.264 or JPEG frames on stdout.

   canonic-window-capture [--jpeg] [--app <bundle id or part of it>] [title]

   The largest visible match wins. Workbench's iOS Simulator lens passes
   `--app simulator`; nothing here is specific to the Simulator. */

private func stderr(_ message: String) {
  FileHandle.standardError.write(Data((message + "\n").utf8))
}

private func appendStartCode(_ data: inout Data) {
  data.append(contentsOf: [0, 0, 0, 1])
}

private final class H264Encoder {
  private var session: VTCompressionSession?
  private var firstTimestamp: CMTime?
  private let output = FileHandle.standardOutput

  init(width: Int32, height: Int32, framesPerSecond: Int32) throws {
    var created: VTCompressionSession?
    let status = VTCompressionSessionCreate(
      allocator: kCFAllocatorDefault,
      width: width,
      height: height,
      codecType: kCMVideoCodecType_H264,
      encoderSpecification: nil,
      imageBufferAttributes: nil,
      compressedDataAllocator: nil,
      outputCallback: { reference, _, status, _, sampleBuffer in
        guard status == noErr, let reference, let sampleBuffer else { return }
        Unmanaged<H264Encoder>.fromOpaque(reference).takeUnretainedValue().write(sampleBuffer)
      },
      refcon: Unmanaged.passUnretained(self).toOpaque(),
      compressionSessionOut: &created
    )
    guard status == noErr, let created else {
      throw NSError(domain: "CanonicWindowCapture", code: Int(status), userInfo: [
        NSLocalizedDescriptionKey: "Could not create the H.264 encoder (VideoToolbox status \(status)).",
      ])
    }
    session = created
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_AllowFrameReordering, value: kCFBooleanFalse)
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_Baseline_AutoLevel)
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: framesPerSecond as CFNumber)
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: framesPerSecond as CFNumber)
    let bitrate = max(2_000_000, Int(width * height * 5)) as CFNumber
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_AverageBitRate, value: bitrate)
    VTCompressionSessionPrepareToEncodeFrames(created)
  }

  deinit {
    if let session {
      VTCompressionSessionCompleteFrames(session, untilPresentationTimeStamp: .invalid)
      VTCompressionSessionInvalidate(session)
    }
  }

  func encode(_ pixelBuffer: CVPixelBuffer, timestamp: CMTime, duration: CMTime) {
    guard let session else { return }
    if firstTimestamp == nil { firstTimestamp = timestamp }
    let relative = CMTimeSubtract(timestamp, firstTimestamp ?? timestamp)
    VTCompressionSessionEncodeFrame(
      session,
      imageBuffer: pixelBuffer,
      presentationTimeStamp: relative,
      duration: duration,
      frameProperties: nil,
      sourceFrameRefcon: nil,
      infoFlagsOut: nil
    )
  }

  private func write(_ sampleBuffer: CMSampleBuffer) {
    guard CMSampleBufferDataIsReady(sampleBuffer),
          let block = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }

    let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false)
      as? [[CFString: Any]]
    let keyFrame = !(attachments?.first?[kCMSampleAttachmentKey_NotSync] as? Bool ?? false)
    var encoded = Data()

    if keyFrame, let format = CMSampleBufferGetFormatDescription(sampleBuffer) {
      for index in 0..<2 {
        var pointer: UnsafePointer<UInt8>?
        var size = 0
        let status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
          format,
          parameterSetIndex: index,
          parameterSetPointerOut: &pointer,
          parameterSetSizeOut: &size,
          parameterSetCountOut: nil,
          nalUnitHeaderLengthOut: nil
        )
        if status == noErr, let pointer, size > 0 {
          appendStartCode(&encoded)
          encoded.append(pointer, count: size)
        }
      }
    }

    var total = 0
    var pointer: UnsafeMutablePointer<Int8>?
    guard CMBlockBufferGetDataPointer(
      block,
      atOffset: 0,
      lengthAtOffsetOut: nil,
      totalLengthOut: &total,
      dataPointerOut: &pointer
    ) == kCMBlockBufferNoErr, let pointer else { return }

    let bytes = UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self)
    var offset = 0
    while offset + 4 <= total {
      let size = Int(bytes[offset]) << 24
        | Int(bytes[offset + 1]) << 16
        | Int(bytes[offset + 2]) << 8
        | Int(bytes[offset + 3])
      offset += 4
      guard size > 0, offset + size <= total else { break }
      appendStartCode(&encoded)
      encoded.append(bytes.advanced(by: offset), count: size)
      offset += size
    }
    guard !encoded.isEmpty else { return }

    let seconds = max(0, CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sampleBuffer)))
    let timestamp = UInt64(seconds * 1_000_000)
    var header = Data()
    var length = UInt32(encoded.count).bigEndian
    var time = timestamp.bigEndian
    withUnsafeBytes(of: &length) { header.append(contentsOf: $0) }
    withUnsafeBytes(of: &time) { header.append(contentsOf: $0) }
    header.append(keyFrame ? 1 : 0)
    output.write(header)
    output.write(encoded)
  }
}

/* VS Code's Electron build does not guarantee an H.264 decoder. JPEG keeps
   the embedded workbench on browser primitives that every webview exposes,
   while the standalone harness can continue exercising the H.264 path. */
private final class JPEGEncoder {
  private var session: VTCompressionSession?
  private var firstTimestamp: CMTime?
  private let output = FileHandle.standardOutput

  init(width: Int32, height: Int32, framesPerSecond: Int32) throws {
    var created: VTCompressionSession?
    let status = VTCompressionSessionCreate(
      allocator: kCFAllocatorDefault,
      width: width,
      height: height,
      codecType: kCMVideoCodecType_JPEG,
      encoderSpecification: nil,
      imageBufferAttributes: nil,
      compressedDataAllocator: nil,
      outputCallback: { reference, _, status, _, sampleBuffer in
        guard status == noErr, let reference, let sampleBuffer else { return }
        Unmanaged<JPEGEncoder>.fromOpaque(reference).takeUnretainedValue().write(sampleBuffer)
      },
      refcon: Unmanaged.passUnretained(self).toOpaque(),
      compressionSessionOut: &created
    )
    guard status == noErr, let created else {
      throw NSError(domain: "CanonicWindowCapture", code: Int(status), userInfo: [
        NSLocalizedDescriptionKey: "Could not create the JPEG encoder (VideoToolbox status \(status)).",
      ])
    }
    session = created
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_Quality, value: 0.72 as CFNumber)
    VTSessionSetProperty(created, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: framesPerSecond as CFNumber)
    VTCompressionSessionPrepareToEncodeFrames(created)
  }

  deinit {
    if let session {
      VTCompressionSessionCompleteFrames(session, untilPresentationTimeStamp: .invalid)
      VTCompressionSessionInvalidate(session)
    }
  }

  func encode(_ pixelBuffer: CVPixelBuffer, timestamp: CMTime, duration: CMTime) {
    guard let session else { return }
    if firstTimestamp == nil { firstTimestamp = timestamp }
    let relative = CMTimeSubtract(timestamp, firstTimestamp ?? timestamp)
    VTCompressionSessionEncodeFrame(
      session,
      imageBuffer: pixelBuffer,
      presentationTimeStamp: relative,
      duration: duration,
      frameProperties: nil,
      sourceFrameRefcon: nil,
      infoFlagsOut: nil
    )
  }

  private func write(_ sampleBuffer: CMSampleBuffer) {
    guard CMSampleBufferDataIsReady(sampleBuffer),
          let block = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }
    var encoded = Data(count: CMBlockBufferGetDataLength(block))
    let copied = encoded.withUnsafeMutableBytes { pointer in
      guard let base = pointer.baseAddress else { return kCMBlockBufferBadCustomBlockSourceErr }
      return CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: pointer.count, destination: base)
    }
    guard copied == kCMBlockBufferNoErr, !encoded.isEmpty else { return }

    let seconds = max(0, CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sampleBuffer)))
    let timestamp = UInt64(seconds * 1_000_000)
    var header = Data()
    var length = UInt32(encoded.count).bigEndian
    var time = timestamp.bigEndian
    withUnsafeBytes(of: &length) { header.append(contentsOf: $0) }
    withUnsafeBytes(of: &time) { header.append(contentsOf: $0) }
    header.append(1)
    output.write(header)
    output.write(encoded)
  }
}

private protocol FrameEncoder: AnyObject {
  func encode(_ pixelBuffer: CVPixelBuffer, timestamp: CMTime, duration: CMTime)
}

extension H264Encoder: FrameEncoder {}
extension JPEGEncoder: FrameEncoder {}

private final class CaptureOutput: NSObject, SCStreamOutput, SCStreamDelegate {
  let encoder: any FrameEncoder
  let duration: CMTime

  init(encoder: any FrameEncoder, framesPerSecond: Int32) {
    self.encoder = encoder
    duration = CMTime(value: 1, timescale: framesPerSecond)
  }

  func stream(
    _ stream: SCStream,
    didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
    of outputType: SCStreamOutputType
  ) {
    guard outputType == .screen, sampleBuffer.isValid,
          let image = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
    encoder.encode(image, timestamp: CMSampleBufferGetPresentationTimeStamp(sampleBuffer), duration: duration)
  }

  func stream(_ stream: SCStream, didStopWithError error: Error) {
    stderr("Capture stopped: \(error.localizedDescription)")
    exit(1)
  }
}

@main
private struct WindowCapture {
  private static var permissionOwner: String {
    let value = ProcessInfo.processInfo.environment["CANONIC_SCREEN_CAPTURE_OWNER"]?
      .trimmingCharacters(in: .whitespacesAndNewlines)
    return value?.isEmpty == false ? value! : "the application running Canonic"
  }

  @MainActor
  private static func initializeAppKit() {
    /* SCShareableContent reaches into CoreGraphics' window server. A command
       line launch does not establish that connection for us, and the
       permission path below only did so accidentally when access was absent.
       VS Code commonly has access already, so initialize AppKit explicitly
       before either the preflight or ScreenCaptureKit runs. */
    let application = NSApplication.shared
    application.setActivationPolicy(.accessory)
    application.finishLaunching()
  }

  @MainActor
  private static func requestScreenCapturePermission() -> Bool {
    let application = NSApplication.shared
    application.setActivationPolicy(.regular)
    application.activate(ignoringOtherApps: true)
    let granted = CGRequestScreenCaptureAccess()
    if !granted,
       let settings = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
      NSWorkspace.shared.open(settings)
    }
    return granted
  }

  static func main() async {
    do {
      initializeAppKit()
      if !CGPreflightScreenCaptureAccess() {
        let owner = permissionOwner
        stderr("Requesting Screen Recording permission for \(owner)…")
        guard requestScreenCapturePermission() else {
          throw NSError(domain: "CanonicWindowCapture", code: 1, userInfo: [
            NSLocalizedDescriptionKey: "Enable \(owner) in Screen & System Audio Recording, restart it, then retry.",
          ])
        }
      }

      var arguments = Array(CommandLine.arguments.dropFirst())
      var jpeg = false
      var requestedApp: String?
      while let flag = arguments.first, flag.hasPrefix("--") {
        arguments.removeFirst()
        if flag == "--jpeg" {
          jpeg = true
        } else if flag == "--app", !arguments.isEmpty {
          requestedApp = arguments.removeFirst().lowercased()
        } else {
          throw NSError(domain: "CanonicWindowCapture", code: 3, userInfo: [
            NSLocalizedDescriptionKey: "Unknown argument \(flag).",
          ])
        }
      }
      let requestedTitle = arguments.first?.lowercased()
      let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
      let candidates = content.windows.filter { window in
        let bundle = window.owningApplication?.bundleIdentifier.lowercased() ?? ""
        let title = window.title?.lowercased() ?? ""
        return (requestedApp == nil || bundle.contains(requestedApp!))
          && (requestedTitle == nil || title.contains(requestedTitle!))
      }
      let ordered = candidates.sorted { left, right in
        let leftArea = left.frame.width * left.frame.height
        let rightArea = right.frame.width * right.frame.height
        return leftArea < rightArea
      }
      guard let window = ordered.last else {
        let visible = content.windows.compactMap { window -> String? in
          guard let app = window.owningApplication?.applicationName, let title = window.title, !title.isEmpty else { return nil }
          return "\(app): \(title)"
        }.prefix(12).joined(separator: ", ")
        throw NSError(domain: "CanonicWindowCapture", code: 2, userInfo: [
          NSLocalizedDescriptionKey: "No visible \(requestedApp.map { $0 + " " } ?? "")window was found. Visible windows: \(visible)",
        ])
      }

      let framesPerSecond: Int32 = jpeg ? 20 : 30
      let aspect = window.frame.height / max(window.frame.width, 1)
      let width = min(900, max(2, Int(window.frame.width * 2))) & ~1
      let height = max(2, Int(CGFloat(width) * aspect)) & ~1
      let encoder: any FrameEncoder = jpeg
        ? try JPEGEncoder(width: Int32(width), height: Int32(height), framesPerSecond: framesPerSecond)
        : try H264Encoder(width: Int32(width), height: Int32(height), framesPerSecond: framesPerSecond)
      let output = CaptureOutput(encoder: encoder, framesPerSecond: framesPerSecond)

      let configuration = SCStreamConfiguration()
      configuration.width = width
      configuration.height = height
      configuration.minimumFrameInterval = CMTime(value: 1, timescale: framesPerSecond)
      configuration.queueDepth = 5
      configuration.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
      configuration.showsCursor = false
      configuration.capturesAudio = false

      let filter = SCContentFilter(desktopIndependentWindow: window)
      let stream = SCStream(filter: filter, configuration: configuration, delegate: output)
      try stream.addStreamOutput(
        output,
        type: SCStreamOutputType.screen,
        sampleHandlerQueue: DispatchQueue(label: "canonic.window.capture")
      )
      try await stream.startCapture()
      stderr("Streaming \(window.title ?? "window") as \(jpeg ? "JPEG" : "H.264") at \(width)x\(height), \(framesPerSecond) fps")

      /* async main is already hosted by libdispatch. After the AppKit
         initialization above its continuation runs on the main actor, where
         calling dispatchMain() is an illegal recursive entry and traps with
         SIGILL. Suspend the task instead; the unreachable use keeps the
         capture graph alive for the process lifetime. */
      let lifetime = (stream, output, encoder)
      await withUnsafeContinuation { (_: UnsafeContinuation<Void, Never>) in }
      withExtendedLifetime(lifetime) {}
    } catch {
      stderr("Window capture failed: \(error.localizedDescription)")
      exit(1)
    }
  }
}
