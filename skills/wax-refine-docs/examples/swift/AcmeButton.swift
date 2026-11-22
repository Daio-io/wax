import SwiftUI

/// Visual emphasis for ``AcmeButton``.
public enum AcmeButtonVariant {
    case primary
    case secondary
}

/// Triggers the main action on a screen or sheet.
///
/// Use one primary ``AcmeButton`` per surface and pair it with a secondary
/// variant for alternative actions.
///
/// - Guidance: Use at most one primary button per surface.
/// - Guidance: Prefer verb labels such as "Save" over "OK".
///
/// Example:
/// ```swift
/// AcmeButton("Save") { save() }
/// ```
public struct AcmeButton: View {
    private let title: String
    private let variant: AcmeButtonVariant
    private let isEnabled: Bool
    private let action: () -> Void

    /// Creates a button.
    ///
    /// - Parameters:
    ///   - title: Visible label. Keep it to a short verb phrase such as "Save".
    ///   - variant: Visual emphasis for the action.
    ///   - isEnabled: When false, the button ignores input and renders as disabled.
    ///   - action: Called when the user activates the button.
    public init(
        _ title: String,
        variant: AcmeButtonVariant = .primary,
        isEnabled: Bool = true,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.variant = variant
        self.isEnabled = isEnabled
        self.action = action
    }

    public var body: some View {
        Button(title, action: action)
            .disabled(!isEnabled)
    }
}
