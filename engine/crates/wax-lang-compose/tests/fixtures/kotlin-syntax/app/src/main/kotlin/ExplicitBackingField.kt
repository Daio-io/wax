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

class Item

interface StateFlow<T>

class MutableStateFlow<T>(private val value: T) : StateFlow<T>

@Composable
fun BeforeExplicitBackingField() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}

class ExplicitBackingFieldHolder {
    val state: StateFlow<List<Item>>
        field = MutableStateFlow(emptyList())

    val spacing: Dp
        field = Spacing.small

    val modifier: Modifier
        field = Modifier.padding(7.dp)
}

@Composable
fun AfterExplicitBackingField() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}
