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

interface Scope

class Item

@Composable
fun BeforeAnnotatedFunctionType() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}

val content: @Composable (() -> Unit) = { PrimaryButton(onClick = {}) }
val receiverContent: @Composable (Scope.(Item) -> Unit) = { PrimaryButton(onClick = {}) }
val ordinaryContent: () -> Unit = { PrimaryButton(onClick = {}) }

@Composable
fun AfterAnnotatedFunctionType() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}
