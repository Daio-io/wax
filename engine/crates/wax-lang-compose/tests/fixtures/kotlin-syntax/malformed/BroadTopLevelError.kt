import androidx.compose.runtime.Composable

@Composable
fun BeforeTopLevelGap() {
    PrimaryButton(onClick = {})
}

fun BrokenTopLevel() = ()

@Composable
fun AfterTopLevelGap() {
    PrimaryButton(onClick = {})
    val spacing = Spacing.small
    Box(Modifier.padding(7.dp))
}
