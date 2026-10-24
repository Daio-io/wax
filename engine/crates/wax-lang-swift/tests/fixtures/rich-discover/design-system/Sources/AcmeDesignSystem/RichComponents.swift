import SwiftUI

/// A button used for important actions.
/// - Parameters:
///   - tone: Controls the button's visual emphasis.
///   - disabled: Whether the button ignores interaction.
/// - Guidance: Use quiet for secondary actions.
/// - Guidance: Reserve strong for the primary action.
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
