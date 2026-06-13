import SwiftUI
import DesignSystem

struct ExtendedScreen: View {
    var body: some View {
        VStack {
            PrimaryCTA(
                title: "Alias"
            )
            DesignSystem.Card {
                Text("Qualified")
            }
        }
    }
}
