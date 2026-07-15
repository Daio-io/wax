import SwiftUI
import DesignSystem

struct LocalScreen: View {
    var body: some View {
        VStack(spacing: 12) {
            PrimaryButton(title: "Save")
            DesignSystem.PrimaryCTA(title: "Continue")
            DS.Card {
                Text("Details")
            }
            LocalCard()
            let value = Theme.colors.primary
            Text("Token")
                .foregroundStyle(Color(red: 0.2, green: 0.3, blue: 0.4))
                .font(.system(size: 14))
                .clipShape(RoundedRectangle(cornerRadius: 8))
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
