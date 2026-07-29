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

enum class Filter {
    Latest,
    Unplayed,
    Downloaded,
    InProgress,
}

@Composable
fun BeforeWhenIfBody() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}

@Composable
fun FilteredMessage(filter: Filter, empty: Boolean): String? {
    return when (filter) {
        Filter.Latest, Filter.Unplayed -> null
        Filter.Downloaded -> if (empty) "no downloads" else null
        Filter.InProgress -> if (empty) "nothing in progress" else null
    }
}

@Composable
fun AfterWhenIfBody() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}
