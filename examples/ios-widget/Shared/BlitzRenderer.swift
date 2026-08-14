import UIKit
import CoreGraphics

/// A tappable region extracted from the Blitz DOM: the layout rect (in
/// points) of an HTML element carrying a `data-action` attribute.
struct BlitzHitRegion: Decodable, Identifiable {
    let action: String
    let x: CGFloat
    let y: CGFloat
    let width: CGFloat
    let height: CGFloat
    var id: String { "\(action)@\(x),\(y)" }
}

enum BlitzRenderer {
    /// Render an HTML string to a UIImage of `width` x `height` points at `scale`.
    static func render(html: String, width: CGFloat, height: CGFloat, scale: CGFloat) -> UIImage? {
        renderWithRegions(html: html, width: width, height: height, scale: scale)?.0
    }

    /// Path of the key=value file where Rust persists all widget state.
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

    /// HTML for a widget kind at the current Rust-owned state.
    /// Kinds: `counter`, `counter-lock`, `interactive`, `anim`.
    static func widgetHTML(kind: String, hideTracked: Bool = false, clock: String = "") -> String {
        guard let ptr = blitz_widget_html(statePath, kind, hideTracked ? 1 : 0, clock) else {
            return ""
        }
        defer { blitz_string_free(ptr) }
        return String(cString: ptr)
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

    /// Blitz-rendered ball sprite at `size` points.
    static func ballSprite(size: CGFloat, scale: CGFloat) -> UIImage? {
        guard let ptr = blitz_demo_ball_sprite_html() else { return nil }
        defer { blitz_string_free(ptr) }
        return render(html: String(cString: ptr), width: size, height: size, scale: scale)
    }

    /// Blitz-rendered progress-fill sprite at `width` x `height` points.
    static func fillSprite(width: CGFloat, height: CGFloat, scale: CGFloat) -> UIImage? {
        guard let ptr = blitz_demo_fill_sprite_html() else { return nil }
        defer { blitz_string_free(ptr) }
        return render(html: String(cString: ptr), width: width, height: height, scale: scale)
    }

    /// Render an HTML string and extract the hit regions of all elements
    /// with a `data-action` attribute (rects in points).
    /// `time` is the CSS animation clock (seconds): the render samples every
    /// CSS animation/transition at exactly that instant.
    static func renderWithRegions(
        html: String, width: CGFloat, height: CGFloat, scale: CGFloat, time: Double = 0
    ) -> (UIImage, [BlitzHitRegion])? {
        // WidgetKit displaySize can be fractional (e.g. systemMedium is
        // 349.67pt wide). Truncate to whole points BEFORE computing the
        // expected pixel size so it matches what the Rust side renders.
        let w = UInt32(width)
        let h = UInt32(height)
        var len: Int = 0
        var regionsJSON: UnsafeMutablePointer<CChar>? = nil
        guard let ptr = html.withCString({ cstr in
            blitz_render_html_with_regions(cstr, w, h, Float(scale), time, &len, &regionsJSON)
        }) else { return nil }
        defer { blitz_buffer_free(ptr, len) }

        var regions: [BlitzHitRegion] = []
        if let regionsJSON {
            defer { blitz_string_free(regionsJSON) }
            let json = String(cString: regionsJSON)
            if let data = json.data(using: .utf8),
               let parsed = try? JSONDecoder().decode([BlitzHitRegion].self, from: data) {
                regions = parsed
            }
        }

        let pw = Int(CGFloat(w) * scale)
        let ph = Int(CGFloat(h) * scale)
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

        return (UIImage(cgImage: cgImage, scale: scale, orientation: .up), regions)
    }
}
