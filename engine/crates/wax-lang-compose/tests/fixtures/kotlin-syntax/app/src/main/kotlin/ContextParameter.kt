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

class ItemScope

@Composable
fun BeforeContextParameter() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}

context(itemScope: ItemScope)
@Composable
fun ContextScreen() { PrimaryButton(onClick = {}) }

@Composable
fun AfterContextParameter() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}
