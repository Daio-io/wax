import SwiftUI
import DesignSystem

struct LocalScreen: View {
    var body: some View {
        let ordinary = 200
        VStack(spacing: 4) {
            PrimaryButton(title: "Save")
            DesignSystem.PrimaryCTA(title: "Continue")
            DS.Card {
                Text("Details")
            }
            LocalCard()
            let value = Theme.colors.primary
            Text("Token")
                .foregroundStyle(Color(red: 0.2, green: 0.3, blue: 0.4))
                .padding(4)
                .frame(width: 200, height: 40)
                .font(.system(size: 4))
                .clipShape(RoundedRectangle(cornerRadius: 4))
                .cornerRadius(4)
                .shadow(radius: 4)
            Text("\(ordinary)")
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
