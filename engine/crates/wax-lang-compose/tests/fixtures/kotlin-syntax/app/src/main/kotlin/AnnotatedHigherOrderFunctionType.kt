@Target(AnnotationTarget.TYPE, AnnotationTarget.FUNCTION)
annotation class Composable

class Dp(private val value: Int)
val Int.dp: Dp get() = Dp(this)
object Spacing { val small = 4.dp }
object Modifier { fun padding(value: Dp): Modifier = this }
fun PrimaryButton(onClick: () -> Unit, modifier: Modifier = Modifier) { onClick(); modifier.hashCode() }
enum class PlayerDisplay { Main, SpeedControls }
val PlayerDisplay.showsParentDoneAction: Boolean get() = this == PlayerDisplay.SpeedControls
class PlayerControlsHostState(val display: PlayerDisplay, val podcastSlug: String?) { fun close() = Unit }

@Composable
fun BeforeAnnotatedHigherOrderFunctionType() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}

@Composable
internal fun PlayerControlsHostState.SpeedControlsContent(
    podcastSlug: String?,
    content: @Composable (onDone: () -> Unit) -> Unit,
) { if (display == PlayerDisplay.SpeedControls && this.podcastSlug == podcastSlug) content(::close) }

@Composable
internal fun PlayerControlsHostState.ParentDoneAction(content: @Composable (onDone: () -> Unit) -> Unit) {
    if (display.showsParentDoneAction) content(::close)
}

fun multipleCallbacks(content: @Composable (onDone: () -> Unit, onCancel: () -> Unit) -> Unit) = Unit
fun unnamedCallback(content: @Composable (() -> Unit) -> Unit) = Unit
fun nullableCallback(content: @Composable (onDone: (() -> Unit)?) -> Unit) = Unit
class HigherOrderSlots(val content: @Composable (onDone: () -> Unit) -> Unit)
val higherOrderContent: @Composable (onDone: () -> Unit) -> Unit = { onDone ->
    PrimaryButton(onClick = onDone)
}
fun higherOrderFactory(): @Composable (onDone: () -> Unit) -> Unit = { onDone ->
    PrimaryButton(onClick = onDone)
}

@Composable
fun AfterAnnotatedHigherOrderFunctionType() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}
