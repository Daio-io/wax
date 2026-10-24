package com.example.ds

import androidx.compose.runtime.Composable

enum class Style { strong, quiet }

/**
 * A button used for important actions.
 *
 * @param text Visible button label.
 * @param enabled Whether the button accepts input.
 * @param onClick Invoked when the button is pressed.
 * @param style Visual emphasis.
 * @guidance Prefer touch-first controls.
 * @guidance Keep labels concise.
 * @deprecated Prefer ActionButton instead.
 * @sample com.example.ds.samples.RichButtonSample
 *
 * ```
 * RichButton(text = "Go") {}
 * ```
 */
@Deprecated("Prefer ActionButton instead.")
@Composable
fun RichButton(
    text: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
    style: Style = Style.strong,
) {
}
