import SwiftUI

@main
struct BlitzWidgetDemoApp: App {
    var body: some Scene {
        WindowGroup {
            VStack(spacing: 16) {
                Text("Blitz Widgets").font(.largeTitle).bold()
                Text("Long-press the home screen and add the \"Blitz Counter\" widget. The widget content is HTML/CSS rendered by the Blitz engine.")
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)
            }
        }
    }
}
