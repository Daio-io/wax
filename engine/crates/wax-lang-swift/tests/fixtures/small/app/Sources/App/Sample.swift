import SwiftUI
import DesignSystem

struct LocalScreen: View {
    var body: some View {
        VStack {
            PrimaryButton(title: "Save")
            DesignSystem.PrimaryCTA(title: "Continue")
            DS.Card {
                Text("Details")
            }
            LocalCard()
        }
    }
}

struct LocalCard: View {
    var body: some View {
        Text("Local")
    }
}

func LocalFactory() -> some View {
    Card {
        Text("Factory")
    }
}
