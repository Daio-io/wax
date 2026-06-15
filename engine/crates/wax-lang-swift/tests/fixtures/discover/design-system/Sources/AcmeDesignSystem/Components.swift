import SwiftUI

public struct PrimaryButton: View {
    public var body: some View { Text("Button") }
}

struct PackageCard: View {
    var body: some View { Text("Card") }
}

public func Badge() -> some View {
    Text("Badge")
}

private struct PrivateTokenView: View {
    var body: some View { Text("Private") }
}

fileprivate func FilePrivateThing() -> some View {
    Text("No")
}

struct lowercase: View {
    var body: some View { Text("No") }
}
