import WidgetKit
import SwiftUI
import AppIntents

// All widget state (counters, slider, animation clock, playback) is owned
// and persisted by Rust (blitz-widget-ffi's store). Swift is a dumb shell:
// intents forward the tapped element's `data-action` to Rust, providers ask
// Rust for the HTML/timeline of a widget kind and blit the rendered frames.

// MARK: - App Intents (run inside the widget extension, forward to Rust)

struct IncrementIntent: AppIntent {
    static var title: LocalizedStringResource = "Increment Blitz Counter"
    static var description = IntentDescription("Increments the Blitz widget counter.")

    func perform() async throws -> some IntentResult {
        BlitzRenderer.dispatch("count")
        return .result()
    }
}

/// One AppIntent carrying the `data-action` of the tapped HTML element.
struct WidgetActionIntent: AppIntent {
    static var title: LocalizedStringResource = "Blitz Widget Action"
    static var description = IntentDescription("Dispatches an action to a Blitz HTML element.")

    @Parameter(title: "Action") var action: String

    init() {}
    init(action: String) { self.action = action }

    func perform() async throws -> some IntentResult {
        BlitzRenderer.dispatch(action)
        return .result()
    }
}

// MARK: - Timeline

struct BlitzEntry: TimelineEntry {
    let date: Date
    let image: UIImage?
}

struct BlitzProvider: TimelineProvider {
    private func makeImage(context: Context, date: Date) -> UIImage? {
        let size = context.displaySize
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm"
        let html: String
        switch context.family {
        case .accessoryRectangular, .accessoryCircular, .accessoryInline:
            html = BlitzRenderer.widgetHTML(kind: "counter-lock")
        default:
            html = BlitzRenderer.widgetHTML(kind: "counter", clock: formatter.string(from: date))
        }
        return BlitzRenderer.render(html: html, width: size.width, height: size.height, scale: 2.0)
    }

    func placeholder(in context: Context) -> BlitzEntry {
        BlitzEntry(date: Date(), image: makeImage(context: context, date: Date()))
    }

    func getSnapshot(in context: Context, completion: @escaping (BlitzEntry) -> Void) {
        completion(BlitzEntry(date: Date(), image: makeImage(context: context, date: Date())))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<BlitzEntry>) -> Void) {
        let now = Date()
        let entry = BlitzEntry(date: now, image: makeImage(context: context, date: now))
        completion(Timeline(entries: [entry], policy: .after(now.addingTimeInterval(60))))
    }
}

// MARK: - Widget views

struct BlitzWidgetView: View {
    @Environment(\.widgetFamily) var family
    let entry: BlitzEntry

    var body: some View {
        Group {
            if let image = entry.image {
                Button(intent: IncrementIntent()) {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFill()
                }
                .buttonStyle(.plain)
            } else {
                Text("Blitz render failed")
            }
        }
        .containerBackground(for: .widget) { Color.clear }
    }
}

// MARK: - Interactive multi-region widget (counter + slider)

struct BlitzDemoEntry: TimelineEntry {
    let date: Date
    let image: UIImage?
    let regions: [BlitzHitRegion]
}

struct BlitzDemoProvider: TimelineProvider {
    private func makeEntry(context: Context, date: Date) -> BlitzDemoEntry {
        let size = context.displaySize
        let html = BlitzRenderer.widgetHTML(kind: "interactive")
        let rendered = BlitzRenderer.renderWithRegions(
            html: html, width: size.width, height: size.height, scale: 2.0)
        return BlitzDemoEntry(date: date, image: rendered?.0, regions: rendered?.1 ?? [])
    }

    func placeholder(in context: Context) -> BlitzDemoEntry {
        makeEntry(context: context, date: Date())
    }

    func getSnapshot(in context: Context, completion: @escaping (BlitzDemoEntry) -> Void) {
        completion(makeEntry(context: context, date: Date()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<BlitzDemoEntry>) -> Void) {
        let now = Date()
        let entry = makeEntry(context: context, date: now)
        completion(Timeline(entries: [entry], policy: .after(now.addingTimeInterval(3600))))
    }
}

/// Displays the Blitz-rendered bitmap with one invisible `Button(intent:)`
/// overlaid per `data-action` element rect extracted from the Blitz DOM, so
/// individual HTML elements act as separate tap targets.
struct BlitzDemoWidgetView: View {
    let entry: BlitzDemoEntry

    var body: some View {
        Group {
            if let image = entry.image {
                ZStack(alignment: .topLeading) {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFill()
                    ForEach(entry.regions) { region in
                        Button(intent: WidgetActionIntent(action: region.action)) {
                            Color.clear
                                .frame(width: region.width, height: region.height)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .offset(x: region.x, y: region.y)
                    }
                }
            } else {
                Text("Blitz render failed")
            }
        }
        .containerBackground(for: .widget) { Color.clear }
    }
}

struct BlitzDemoWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "BlitzDemoWidget", provider: BlitzDemoProvider()) { entry in
            BlitzDemoWidgetView(entry: entry)
        }
        .configurationDisplayName("Blitz Interactive")
        .description("Counter + slider: every HTML element is its own tap target, rendered by Blitz.")
        .supportedFamilies([.systemMedium])
        .contentMarginsDisabled()
    }
}

// MARK: - CSS animation widget (we control the animation clock)

/// Blitz resolves styles with an explicit `current_time_for_animations`, so
/// every render samples the document's CSS `@keyframes` animations at exactly
/// the instant Rust chooses. The scrubber, Step, and Play actions all mutate
/// the Rust-owned animation clock via `dispatch`.
struct AnimActionIntent: AppIntent {
    static var title: LocalizedStringResource = "Blitz Animation Scrub"
    static var description = IntentDescription("Moves the Blitz CSS animation clock.")

    @Parameter(title: "Action") var action: String

    init() {}
    init(action: String) { self.action = action }

    func perform() async throws -> some IntentResult {
        BlitzRenderer.dispatch(action)
        return .result()
    }
}

/// Entry for the animation widget: the card background is one Blitz bitmap
/// (tracked elements hidden), while the ball and progress fill are separate
/// Blitz-rendered sprite layers positioned from `data-track` rects resolved
/// at the sampled animation time. Because the layers are distinct SwiftUI
/// views whose frames/offsets change between renders, WidgetKit tweens their
/// position and size with its spring at full frame rate — CSS keyframe motion
/// glides instead of cross-fading.
struct BlitzAnimEntry: TimelineEntry {
    var date: Date
    let background: UIImage?
    let ballSprite: UIImage?
    let fillSprite: UIImage?
    let ballRect: CGRect
    let fillRect: CGRect
    let railRect: CGRect
    let buttons: [BlitzHitRegion]
}

struct BlitzAnimProvider: TimelineProvider {
    /// The sprites are identical for every frame, so render each once per
    /// process and reuse it across flip-book entries.
    private static var spriteCache: [String: UIImage] = [:]

    private static func cachedSprite(key: String, render: () -> UIImage?) -> UIImage? {
        if let cached = spriteCache[key] { return cached }
        let sprite = render()
        if let sprite { spriteCache[key] = sprite }
        return sprite
    }

    private func makeEntry(context: Context, date: Date, animTime: Double) -> BlitzAnimEntry {
        let size = context.displaySize
        let html = BlitzRenderer.widgetHTML(kind: "anim", hideTracked: true)
        let rendered = BlitzRenderer.renderWithRegions(
            html: html, width: size.width, height: size.height, scale: 2.0,
            time: animTime)
        let regions = rendered?.1 ?? []

        func rect(_ track: String) -> CGRect {
            guard let r = regions.first(where: { $0.action == "track:\(track)" }) else {
                return .zero
            }
            return CGRect(x: r.x, y: r.y, width: r.width, height: r.height)
        }

        let ballRect = rect("ball")
        let railRect = rect("rail")
        return BlitzAnimEntry(
            date: date,
            background: rendered?.0,
            ballSprite: Self.cachedSprite(key: "ball") {
                BlitzRenderer.ballSprite(size: 51, scale: 2.0)
            },
            fillSprite: Self.cachedSprite(key: "fill:\(railRect.size)") {
                BlitzRenderer.fillSprite(
                    width: max(railRect.width, 1), height: max(railRect.height, 1), scale: 2.0)
            },
            ballRect: ballRect,
            fillRect: rect("fill"),
            railRect: railRect,
            buttons: regions.filter { !$0.action.hasPrefix("track:") }
        )
    }

    func placeholder(in context: Context) -> BlitzAnimEntry {
        makeEntry(context: context, date: Date(), animTime: blitz_widget_anim_time(BlitzRenderer.statePath))
    }

    func getSnapshot(in context: Context, completion: @escaping (BlitzAnimEntry) -> Void) {
        completion(makeEntry(
            context: context, date: Date(),
            animTime: blitz_widget_anim_time(BlitzRenderer.statePath)))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<BlitzAnimEntry>) -> Void) {
        // Rust plans the timeline: one frame at the current clock normally, a
        // flip-book (one frame per second of playback) right after "play".
        // WidgetKit shows each frame at its date and tweens the sprite layers
        // between consecutive frames. Render every frame BEFORE dating the
        // entries: the renders take a while, and entries dated from the
        // pre-render clock end up in the past, delaying playback. The first
        // frame's negative offset backdates the rest pose so the first moving
        // frame lands just after "now".
        var frames = BlitzRenderer.animTimeline()
        if frames.isEmpty {
            frames = [.init(offset: 0, time: blitz_widget_anim_time(BlitzRenderer.statePath))]
        }
        var entries = frames.map { frame in
            makeEntry(context: context, date: .distantPast, animTime: frame.time)
        }
        let start = Date()
        for i in entries.indices {
            entries[i].date = start.addingTimeInterval(frames[i].offset)
        }
        completion(Timeline(entries: entries, policy: .never))
    }
}

struct BlitzAnimWidgetView: View {
    let entry: BlitzAnimEntry

    var body: some View {
        Group {
            if let background = entry.background {
                ZStack(alignment: .topLeading) {
                    Image(uiImage: background)
                        .resizable()
                        .scaledToFill()
                    if let fill = entry.fillSprite, entry.railRect.width > 0 {
                        Image(uiImage: fill)
                            .resizable()
                            .frame(width: entry.railRect.width, height: entry.railRect.height)
                            .mask(alignment: .leading) {
                                Rectangle().frame(width: max(entry.fillRect.width, 0.01))
                            }
                            .offset(x: entry.railRect.minX, y: entry.railRect.minY)
                    }
                    if let ball = entry.ballSprite, entry.ballRect.width > 0 {
                        Image(uiImage: ball)
                            .resizable()
                            .frame(width: entry.ballRect.width, height: entry.ballRect.height)
                            .offset(x: entry.ballRect.minX, y: entry.ballRect.minY)
                    }
                    ForEach(entry.buttons) { region in
                        Button(intent: AnimActionIntent(action: region.action)) {
                            Color.clear
                                .frame(width: region.width, height: region.height)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .offset(x: region.x, y: region.y)
                    }
                }
            } else {
                Text("Blitz render failed")
            }
        }
        .containerBackground(for: .widget) { Color.clear }
    }
}

struct BlitzAnimWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "BlitzAnimWidget", provider: BlitzAnimProvider()) { entry in
            BlitzAnimWidgetView(entry: entry)
        }
        .configurationDisplayName("Blitz CSS Animation")
        .description("CSS keyframe animations sampled at any instant — Blitz controls the animation clock.")
        .supportedFamilies([.systemMedium])
        .contentMarginsDisabled()
    }
}

// MARK: - Widget definitions

struct BlitzCounterWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "BlitzCounterWidget", provider: BlitzProvider()) { entry in
            BlitzWidgetView(entry: entry)
        }
        .configurationDisplayName("Blitz Counter")
        .description("An interactive counter rendered from HTML/CSS by the Blitz engine.")
        .supportedFamilies([.systemSmall, .systemMedium, .accessoryRectangular])
        .contentMarginsDisabled()
    }
}

@main
struct BlitzWidgetBundle: WidgetBundle {
    var body: some Widget {
        BlitzCounterWidget()
        BlitzDemoWidget()
        BlitzAnimWidget()
    }
}
