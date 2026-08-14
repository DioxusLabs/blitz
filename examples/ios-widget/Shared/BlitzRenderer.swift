import UIKit
import CoreGraphics

/// A tappable region from the frame plan: the layout rect (in points) of an
/// HTML element carrying a `data-action` attribute.
struct BlitzHitRegion: Decodable, Identifiable {
    let action: String
    let x: CGFloat
    let y: CGFloat
    let width: CGFloat
    let height: CGFloat
    var id: String { "\(action)@\(x),\(y)" }
}

/// A native compositing layer from the frame plan: draw the `track`'s sprite
/// (rendered at `spriteWidth` x `spriteHeight`) in the layer rect, clipped to
/// `clipWidth` points from the left.
struct BlitzLayer: Decodable {
    let track: String
    let x: CGFloat
    let y: CGFloat
    let width: CGFloat
    let height: CGFloat
    let spriteWidth: CGFloat
    let spriteHeight: CGFloat
    let clipWidth: CGFloat
}

/// Everything Rust tells the shell to composite for one widget frame.
struct BlitzFramePlan: Decodable {
    let buttons: [BlitzHitRegion]
    let layers: [BlitzLayer]
}

/// Thin transport over the Rust-owned widget: Rust holds all state, handles
/// every action, and decides what is rendered where; Swift only shuffles
/// frames and events back and forth.
enum BlitzRenderer {
    /// Path of the file where Rust persists all widget state.
    /// Swift only supplies the platform location; it never reads or writes it.
    static let statePath: String = {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSTemporaryDirectory())
        return dir.appendingPathComponent("blitz-widget-state.txt").path
    }()

    /// Forward a tapped element's `data-action` to the Rust-owned state store.
    static func dispatch(_ action: String) {
        blitz_widget_dispatch(statePath, action)
    }

    /// One frame of the animation widget's timeline, planned by Rust.
    struct BlitzAnimFrame: Decodable {
        /// Seconds relative to now at which to show the frame.
        let offset: Double
        /// The CSS animation clock to render the frame at.
        let time: Double
    }

    private struct BlitzAnimTimeline: Decodable {
        let frames: [BlitzAnimFrame]
    }

    /// The animation widget's timeline plan (one frame normally, a flip-book
    /// right after a `play` action), decided entirely by Rust.
    static func animTimeline() -> [BlitzAnimFrame] {
        guard let ptr = blitz_widget_anim_timeline_json(statePath) else { return [] }
        defer { blitz_string_free(ptr) }
        let json = String(cString: ptr)
        guard let data = json.data(using: .utf8),
              let parsed = try? JSONDecoder().decode(BlitzAnimTimeline.self, from: data) else {
            return []
        }
        return parsed.frames
    }

    /// One complete widget frame at the current Rust-owned state: the
    /// background image plus the plan of layers and buttons to composite.
    /// Kinds: `counter`, `counter-lock`, `interactive`, `anim`.
    /// `time` is the CSS animation clock (seconds) to sample the frame at.
    static func widgetFrame(
        kind: String, width: CGFloat, height: CGFloat, scale: CGFloat,
        time: Double = 0, hideTracked: Bool = false, clock: String = ""
    ) -> (UIImage, BlitzFramePlan)? {
        // WidgetKit displaySize can be fractional (e.g. systemMedium is
        // 349.67pt wide). Truncate to whole points BEFORE computing the
        // expected pixel size so it matches what the Rust side renders.
        let w = UInt32(width)
        let h = UInt32(height)
        var len: Int = 0
        var planJSON: UnsafeMutablePointer<CChar>? = nil
        guard let ptr = blitz_widget_frame(
            statePath, kind, w, h, Float(scale), time, hideTracked ? 1 : 0, clock,
            &len, &planJSON
        ) else { return nil }
        defer { blitz_buffer_free(ptr, len) }

        var plan = BlitzFramePlan(buttons: [], layers: [])
        if let planJSON {
            defer { blitz_string_free(planJSON) }
            let json = String(cString: planJSON)
            if let data = json.data(using: .utf8),
               let parsed = try? JSONDecoder().decode(BlitzFramePlan.self, from: data) {
                plan = parsed
            }
        }

        guard let image = makeImage(ptr: ptr, len: len, width: w, height: h, scale: scale) else {
            return nil
        }
        return (image, plan)
    }

    /// Sprites are identical for every frame, so render each track+size once
    /// per process and reuse it across flip-book entries.
    private static var spriteCache: [String: UIImage] = [:]

    /// The Blitz-rendered sprite of a `data-track` layer from the frame plan.
    static func sprite(track: String, width: CGFloat, height: CGFloat, scale: CGFloat) -> UIImage? {
        let w = UInt32(max(width, 1))
        let h = UInt32(max(height, 1))
        let key = "\(track)@\(w)x\(h)"
        if let cached = spriteCache[key] { return cached }
        var len: Int = 0
        guard let ptr = blitz_widget_sprite(track, w, h, Float(scale), &len) else { return nil }
        defer { blitz_buffer_free(ptr, len) }
        let image = makeImage(ptr: ptr, len: len, width: w, height: h, scale: scale)
        if let image { spriteCache[key] = image }
        return image
    }

    /// Wrap a Rust RGBA8888 buffer of `width` x `height` points at `scale`
    /// into a UIImage.
    private static func makeImage(
        ptr: UnsafeMutablePointer<UInt8>, len: Int,
        width: UInt32, height: UInt32, scale: CGFloat
    ) -> UIImage? {
        let pw = Int(CGFloat(width) * scale)
        let ph = Int(CGFloat(height) * scale)
        guard len == pw * ph * 4 else { return nil }

        let data = Data(bytes: ptr, count: len)
        guard let provider = CGDataProvider(data: data as CFData) else { return nil }
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
        guard let cgImage = CGImage(
            width: pw,
            height: ph,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: pw * 4,
            space: colorSpace,
            bitmapInfo: bitmapInfo,
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        ) else { return nil }

        return UIImage(cgImage: cgImage, scale: scale, orientation: .up)
    }
}
