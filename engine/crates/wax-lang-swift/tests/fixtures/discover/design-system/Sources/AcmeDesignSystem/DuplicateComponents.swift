import SwiftUI

public struct PrimaryButton: View {
    public var body: some View { Text("Duplicate") }
}

enum NestedNamespace {
    struct NestedCard: View {
        var body: some View { Text("Nested") }
    }
}
