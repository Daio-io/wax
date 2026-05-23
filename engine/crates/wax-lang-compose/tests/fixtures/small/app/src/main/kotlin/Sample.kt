package com.example.app

import com.example.ds.PrimaryButton
import com.example.ds.TextField

@Composable
fun LocalCard(content: @Composable () -> Unit) {
    PrimaryButton(onClick = {})
    content()
}

@Composable
fun SampleScreen() {
    PrimaryButton(onClick = {})
    TextField(value = "", onValueChange = {})
}
