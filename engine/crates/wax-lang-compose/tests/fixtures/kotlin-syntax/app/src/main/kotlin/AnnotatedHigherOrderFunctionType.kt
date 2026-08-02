@Target(AnnotationTarget.TYPE, AnnotationTarget.FUNCTION)
annotation class Composable

class Dp(private val value: Int)

val Int.dp: Dp
    get() = Dp(this)

object Spacing {
    val small = 4.dp
}

object Modifier {
    fun padding(value: Dp): Modifier = this
}

fun PrimaryButton(onClick: () -> Unit, modifier: Modifier = Modifier) {
    onClick()
    modifier.hashCode()
}

@Composable
fun BeforeAnnotatedHigherOrderFunctionType() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}

fun host(content: @Composable (onDone: () -> Unit) -> Unit) = Unit

val higherOrderContent: @Composable (onDone: () -> Unit) -> Unit
    = { onDone ->
        PrimaryButton(onClick = onDone)
    }

fun higherOrderFactory(): @Composable (onDone: () -> Unit) -> Unit
    = { onDone ->
        PrimaryButton(onClick = onDone)
    }

@Composable
fun AfterAnnotatedHigherOrderFunctionType() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}
