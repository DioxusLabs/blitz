import WidgetKit
import SwiftUI
import AppIntents

// MARK: - Shared counter state (persisted in the widget extension's process)

enum CounterStore {
    static let key = "blitz.counter"
    static var count: Int {
        get { UserDefaults.standard.integer(forKey: key) }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }
}

// MARK: - Interactive App Intent (runs inside the widget extension)

struct IncrementIntent: AppIntent {
    static var title: LocalizedStringResource = "Increment Blitz Counter"
    static var description = IntentDescription("Increments the Blitz widget counter.")

    func perform() async throws -> some IntentResult {
        CounterStore.count += 1
        return .result()
    }
}

// MARK: - HTML templates rendered by Blitz

func homeScreenHTML(count: Int, time: String) -> String {
    """
    <!DOCTYPE html>
    <html><head><style>
      body { margin: 0; font-family: sans-serif; }
      .card {
        box-sizing: border-box; width: 100%; height: 100vh; padding: 14px;
        background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
        color: white; display: flex; flex-direction: column;
        justify-content: space-between;
      }
      .row { display: flex; justify-content: space-between; align-items: center; }
      .title { font-size: 13px; font-weight: 600; opacity: 0.9; }
      .time { font-size: 11px; opacity: 0.7; }
      .count { font-size: 48px; font-weight: bold; text-align: center; }
      .hint {
        font-size: 11px; text-align: center; opacity: 0.85;
        background: rgba(255,255,255,0.18); border-radius: 10px; padding: 5px 8px;
      }
    </style></head>
    <body><div class="card">
      <div class="row">
        <div class="title">⚡ Blitz Counter</div>
        <div class="time">\(time)</div>
      </div>
      <div class="count">\(count)</div>
      <div class="hint">Tap to increment · HTML by Blitz</div>
    </div></body></html>
    """
}

func lockScreenHTML(count: Int) -> String {
    """
    <!DOCTYPE html>
    <html><head><style>
      body { margin: 0; font-family: sans-serif; }
      .card {
        box-sizing: border-box; width: 100%; height: 100vh; padding: 8px 12px;
        color: white; display: flex; align-items: center; gap: 10px;
      }
      .count { font-size: 32px; font-weight: bold; }
      .label { font-size: 12px; line-height: 1.3; opacity: 0.9; }
    </style></head>
    <body><div class="card">
      <div class="count">\(count)</div>
      <div class="label">Blitz Counter<br>HTML/CSS render</div>
    </div></body></html>
    """
}

// MARK: - Interactive demo state (counter + slider) and per-element actions

enum DemoStore {
    static let countKey = "blitz.demo.count"
    static let sliderKey = "blitz.demo.slider"
    static var count: Int {
        get { UserDefaults.standard.integer(forKey: countKey) }
        set { UserDefaults.standard.set(newValue, forKey: countKey) }
    }
    static var slider: Int {
        get {
            UserDefaults.standard.object(forKey: sliderKey) == nil
                ? 5 : UserDefaults.standard.integer(forKey: sliderKey)
        }
        set { UserDefaults.standard.set(newValue, forKey: sliderKey) }
    }

    static func apply(_ action: String) {
        switch action {
        case "incr": count += 1
        case "decr": count -= 1
        case "reset": count = 0; slider = 5
        default:
            if action.hasPrefix("slider:"), let value = Int(action.dropFirst(7)) {
                slider = max(0, min(10, value))
            }
        }
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
        DemoStore.apply(action)
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
        let count = CounterStore.count
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm"
        let html: String
        switch context.family {
        case .accessoryRectangular, .accessoryCircular, .accessoryInline:
            html = lockScreenHTML(count: count)
        default:
            html = homeScreenHTML(count: count, time: formatter.string(from: date))
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
        let html = BlitzRenderer.demoHTML(count: DemoStore.count, slider: DemoStore.slider)
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
/// the instant we choose. The scrubber and Step button pick that instant.
enum AnimStore {
    static let timeKey = "blitz.anim.time"
    static let playKey = "blitz.anim.playStart"
    static let duration: Double = 4.0
    /// Seconds of flip-book playback triggered by the `play` action.
    static let playLength: Double = 8.0

    static var time: Double {
        get { UserDefaults.standard.double(forKey: timeKey) }
        set { UserDefaults.standard.set(newValue, forKey: timeKey) }
    }

    static var playStart: Double {
        get { UserDefaults.standard.double(forKey: playKey) }
        set { UserDefaults.standard.set(newValue, forKey: playKey) }
    }

    static var scrubSegment: Int {
        Int((time / duration * 10).rounded())
    }

    static func apply(_ action: String) {
        if action == "step" {
            time = (time + 0.4).truncatingRemainder(dividingBy: duration + 0.0001)
        } else if action == "play" {
            playStart = Date().timeIntervalSince1970
        } else if action.hasPrefix("time:"), let seg = Int(action.dropFirst(5)) {
            time = Double(max(0, min(10, seg))) / 10.0 * duration
        }
    }
}

struct AnimActionIntent: AppIntent {
    static var title: LocalizedStringResource = "Blitz Animation Scrub"
    static var description = IntentDescription("Moves the Blitz CSS animation clock.")

    @Parameter(title: "Action") var action: String

    init() {}
    init(action: String) { self.action = action }

    func perform() async throws -> some IntentResult {
        AnimStore.apply(action)
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
        let html = BlitzRenderer.animatedHTML(scrub: AnimStore.scrubSegment, hideTracked: true)
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
        makeEntry(context: context, date: Date(), animTime: AnimStore.time)
    }

    func getSnapshot(in context: Context, completion: @escaping (BlitzAnimEntry) -> Void) {
        completion(makeEntry(context: context, date: Date(), animTime: AnimStore.time))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<BlitzAnimEntry>) -> Void) {
        let now = Date()
        let sincePlay = now.timeIntervalSince1970 - AnimStore.playStart
        if AnimStore.playStart > 0 && sincePlay < AnimStore.playLength {
            // Flip-book playback: pre-render one entry per second, each
            // sampling the CSS animations one second further along the clock.
            // WidgetKit shows each at its date and tweens the sprite layers
            // between consecutive frames. Render every frame BEFORE dating
            // the entries: the renders take a while, and entries dated from
            // the pre-render clock end up in the past, delaying playback.
            let base = AnimStore.time
            var entries: [BlitzAnimEntry] = []
            for i in 0...Int(AnimStore.playLength) {
                let t = (base + Double(i)).truncatingRemainder(dividingBy: AnimStore.duration)
                entries.append(makeEntry(context: context, date: .distantPast, animTime: t))
            }
            // Entry 0 is the current rest pose the widget is already showing,
            // so date it in the past: WidgetKit displays it immediately and
            // the first moving frame (entry 1) lands just after "now".
            let start = Date().addingTimeInterval(-0.8)
            for i in entries.indices {
                entries[i].date = start.addingTimeInterval(Double(i))
            }
            completion(Timeline(entries: entries, policy: .never))
        } else {
            let entry = makeEntry(context: context, date: now, animTime: AnimStore.time)
            completion(Timeline(entries: [entry], policy: .never))
        }
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
