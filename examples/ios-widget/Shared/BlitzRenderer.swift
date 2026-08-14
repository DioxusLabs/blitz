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

    /// Build the shared demo widget HTML (counter + slider) from Rust.
    static func demoHTML(count: Int, slider: Int) -> String {
        guard let ptr = blitz_demo_widget_html(Int32(count), Int32(slider)) else { return "" }
        defer { blitz_string_free(ptr) }
        return String(cString: ptr)
    }

    /// Build the animated demo widget HTML (CSS keyframes + scrubber) from Rust.
    /// With `hideTracked` the `data-track` elements (ball, fill) keep their
    /// layout but don't paint, so they can be composited as native layers.
    static func animatedHTML(scrub: Int, hideTracked: Bool = false) -> String {
        guard let ptr = blitz_demo_animated_html(Int32(scrub), hideTracked ? 1 : 0) else {
            return ""
        }
        defer { blitz_string_free(ptr) }
        return String(cString: ptr)
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
