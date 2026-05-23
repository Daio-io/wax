package com.example.app

import com.example.ds.PrimaryButton
import com.example.ds.PrimaryBtn

// Multiline DS call
@Composable
fun MultilineUsageScreen() {
    PrimaryButton(
        onClick = {},
        text = "Click me"
    )
    PrimaryBtn(onClick = {})
}

// Non-DS @Composable — must not emit a resolved DS usage
@Composable
fun CustomCard(content: @Composable () -> Unit) {
    content()
}
