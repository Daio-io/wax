package com.example.ds

import androidx.compose.runtime.Composable

/**
 * A button used for important actions.
 *
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
) {
}
