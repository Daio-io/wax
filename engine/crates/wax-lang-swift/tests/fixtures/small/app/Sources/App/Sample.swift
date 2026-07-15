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
            let primary = Theme.colors.primary
            Text("Token")
                .foregroundStyle(Color(red: 0.2, green: 0.3, blue: 0.4))
                .cornerRadius(8)
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
