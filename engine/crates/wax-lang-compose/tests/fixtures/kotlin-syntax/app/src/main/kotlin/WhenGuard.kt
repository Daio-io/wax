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

open class VisibleItem
class HiddenItem

val featureEnabled = true

@Composable
fun BeforeWhenGuard() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}

@Composable
fun GuardedContent(item: Any) {
    when (item) {
        is VisibleItem if featureEnabled -> PrimaryButton(onClick = {})
        else -> Unit
    }
}

@Composable
fun AfterWhenGuard() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}
