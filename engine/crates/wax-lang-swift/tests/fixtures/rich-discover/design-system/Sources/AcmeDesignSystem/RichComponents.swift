import SwiftUI

/// A button used for important actions.
///
/// Example:
/// ```swift
/// RichButton(tone: .quiet, disabled: false)
/// ```
@available(*, deprecated, message: "Use ActionButton instead.")
public struct RichButton: View {
    public init(tone: Tone = .strong, disabled: Bool) {}

    public var body: some View {
        Text("Rich")
    }
}

public enum Tone {
    case strong
    case quiet
}
