package com.example.ds

import androidx.compose.runtime.Composable

@Composable
fun PrimaryButton() {}

@Composable
public fun SecondaryButton() {}

@androidx.compose.runtime.Composable
fun QualifiedButton() {}

@Composable
internal fun InternalButton() {}

@Composable
private fun PrivateButton() {}

@Composable
fun helperText() {}

object Cards {
    @Composable
    fun NestedCard() {}
}

fun NotComposable() {}
