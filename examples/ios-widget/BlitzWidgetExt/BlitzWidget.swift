import WidgetKit
import SwiftUI
import AppIntents

// Rust owns the whole widget: all state, every action, and what is rendered
// where. Each provider asks Rust for a complete frame — background pixels
// plus a plan of sprite layers and button rects — and the generic view below
// composites exactly what the plan says. Swift only shuffles frames and
// events back and forth.

// MARK: - App Intent (forwards the tapped element's data-action to Rust)

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

// MARK: - Generic frame entry and view

/// A sprite layer resolved from the frame plan, ready to composite.
struct BlitzLayerImage: Identifiable {
    let track: String
    let image: UIImage
    let rect: CGRect
    let clipWidth: CGFloat
    var id: String { track }
}

/// One Rust-planned widget frame: the background bitmap, the sprite layers to
/// draw over it, and the invisible tap targets.
struct BlitzFrameEntry: TimelineEntry {
    var date: Date
    let background: UIImage?
    let layers: [BlitzLayerImage]
    let buttons: [BlitzHitRegion]
}

/// Build an entry by asking Rust for a complete frame and rendering the
/// sprite of each planned layer.
func makeFrameEntry(
    kind: String, context: TimelineProviderContext, date: Date,
    time: Double = 0, hideTracked: Bool = false, clock: String = ""
) -> BlitzFrameEntry {
    let size = context.displaySize
    guard let (background, plan) = BlitzRenderer.widgetFrame(
        kind: kind, width: size.width, height: size.height, scale: 2.0,
        time: time, hideTracked: hideTracked, clock: clock
    ) else {
        return BlitzFrameEntry(date: date, background: nil, layers: [], buttons: [])
    }
    let layers = plan.layers.compactMap { layer -> BlitzLayerImage? in
        guard let sprite = BlitzRenderer.sprite(
            track: layer.track, width: layer.spriteWidth, height: layer.spriteHeight, scale: 2.0
        ) else { return nil }
        return BlitzLayerImage(
            track: layer.track,
            image: sprite,
            rect: CGRect(x: layer.x, y: layer.y, width: layer.width, height: layer.height),
            clipWidth: layer.clipWidth
        )
    }
    return BlitzFrameEntry(date: date, background: background, layers: layers, buttons: plan.buttons)
}

/// Composites exactly what the Rust frame plan says: the background bitmap,
/// one image per sprite layer at its planned rect/clip, and one invisible
/// `Button(intent:)` per planned tap target. Because the layers are distinct
/// SwiftUI views whose frames change between timeline entries, WidgetKit
/// tweens their position and size with its spring at full frame rate — CSS
/// keyframe motion glides instead of cross-fading.
struct BlitzFrameView: View {
    let entry: BlitzFrameEntry

    var body: some View {
        Group {
            if let background = entry.background {
                ZStack(alignment: .topLeading) {
                    Image(uiImage: background)
                        .resizable()
                        .scaledToFill()
                    ForEach(entry.layers) { layer in
                        Image(uiImage: layer.image)
                            .resizable()
                            .frame(width: layer.rect.width, height: layer.rect.height)
                            .mask(alignment: .leading) {
                                Rectangle().frame(width: max(layer.clipWidth, 0.01))
                            }
                            .offset(x: layer.rect.minX, y: layer.rect.minY)
                    }
                    ForEach(entry.buttons) { region in
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

// MARK: - Counter widget (home screen + lock screen)

struct BlitzCounterProvider: TimelineProvider {
    private func makeEntry(context: Context, date: Date) -> BlitzFrameEntry {
        switch context.family {
        case .accessoryRectangular, .accessoryCircular, .accessoryInline:
            return makeFrameEntry(kind: "counter-lock", context: context, date: date)
        default:
            let formatter = DateFormatter()
            formatter.dateFormat = "HH:mm"
            return makeFrameEntry(
                kind: "counter", context: context, date: date,
                clock: formatter.string(from: date))
        }
    }

    func placeholder(in context: Context) -> BlitzFrameEntry {
        makeEntry(context: context, date: Date())
    }

    func getSnapshot(in context: Context, completion: @escaping (BlitzFrameEntry) -> Void) {
        completion(makeEntry(context: context, date: Date()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<BlitzFrameEntry>) -> Void) {
        let now = Date()
        let entry = makeEntry(context: context, date: now)
        completion(Timeline(entries: [entry], policy: .after(now.addingTimeInterval(60))))
    }
}

struct BlitzCounterWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "BlitzCounterWidget", provider: BlitzCounterProvider()) { entry in
            BlitzFrameView(entry: entry)
        }
        .configurationDisplayName("Blitz Counter")
        .description("An interactive counter rendered from HTML/CSS by the Blitz engine.")
        .supportedFamilies([.systemSmall, .systemMedium, .accessoryRectangular])
        .contentMarginsDisabled()
    }
}

// MARK: - Interactive multi-region widget (counter + slider)

struct BlitzDemoProvider: TimelineProvider {
    func placeholder(in context: Context) -> BlitzFrameEntry {
        makeFrameEntry(kind: "interactive", context: context, date: Date())
    }

    func getSnapshot(in context: Context, completion: @escaping (BlitzFrameEntry) -> Void) {
        completion(makeFrameEntry(kind: "interactive", context: context, date: Date()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<BlitzFrameEntry>) -> Void) {
        let now = Date()
        let entry = makeFrameEntry(kind: "interactive", context: context, date: now)
        completion(Timeline(entries: [entry], policy: .after(now.addingTimeInterval(3600))))
    }
}

struct BlitzDemoWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "BlitzDemoWidget", provider: BlitzDemoProvider()) { entry in
            BlitzFrameView(entry: entry)
        }
        .configurationDisplayName("Blitz Interactive")
        .description("Counter + slider: every HTML element is its own tap target, rendered by Blitz.")
        .supportedFamilies([.systemMedium])
        .contentMarginsDisabled()
    }
}

// MARK: - CSS animation widget (Rust controls the animation clock)

/// Blitz resolves styles with an explicit `current_time_for_animations`, so
/// every frame samples the document's CSS `@keyframes` animations at exactly
/// the instant Rust chooses. The scrubber, Step, and Play actions all mutate
/// the Rust-owned animation clock via `dispatch`; the timeline below is
/// planned entirely by Rust.
struct BlitzAnimProvider: TimelineProvider {
    private func makeEntry(context: Context, date: Date, animTime: Double) -> BlitzFrameEntry {
        makeFrameEntry(kind: "anim", context: context, date: date, time: animTime, hideTracked: true)
    }

    func placeholder(in context: Context) -> BlitzFrameEntry {
        makeEntry(
            context: context, date: Date(),
            animTime: blitz_widget_anim_time(BlitzRenderer.statePath))
    }

    func getSnapshot(in context: Context, completion: @escaping (BlitzFrameEntry) -> Void) {
        completion(makeEntry(
            context: context, date: Date(),
            animTime: blitz_widget_anim_time(BlitzRenderer.statePath)))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<BlitzFrameEntry>) -> Void) {
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

struct BlitzAnimWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "BlitzAnimWidget", provider: BlitzAnimProvider()) { entry in
            BlitzFrameView(entry: entry)
        }
        .configurationDisplayName("Blitz CSS Animation")
        .description("CSS keyframe animations sampled at any instant — Blitz controls the animation clock.")
        .supportedFamilies([.systemMedium])
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
