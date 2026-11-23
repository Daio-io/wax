package com.acme.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

/** Visual emphasis for [AcmeButton]. */
enum class AcmeButtonVariant {
    Primary,
    Secondary,
}

/**
 * Triggers the main action on a screen or dialog.
 *
 * Use one primary [AcmeButton] per surface and pair it with a secondary
 * variant for alternative actions.
 *
 * ```kotlin
 * AcmeButton(text = "Save", onClick = { save() })
 * ```
 * @param text Visible label. Keep it to a short verb phrase such as "Save".
 * @param onClick Called when the user activates the button.
 * @param modifier Modifier applied to the button container.
 * @param enabled When false, the button ignores input and renders as disabled.
 * @param variant Visual emphasis for the action.
 * @guidance Use at most one primary
 *   button per surface.
 * @guidance Prefer verb labels such as "Save" over "OK".
 */
@Composable
fun AcmeButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    variant: AcmeButtonVariant = AcmeButtonVariant.Primary,
) {
    AcmeButtonSurface(text, onClick, modifier, enabled, variant)
}
