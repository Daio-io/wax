import kotlin.reflect.KClass

@Target(AnnotationTarget.TYPE, AnnotationTarget.FUNCTION)
annotation class Composable

@Target(AnnotationTarget.TYPE)
annotation class Serializable(val with: KClass<*>)

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

object ItemSerializer

@Composable
fun BeforeAnnotatedTypeArgument() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}

val items: List<
    @Serializable(with = ItemSerializer::class)
    Item,
> = emptyList()

@Composable
fun AfterAnnotatedTypeArgument() {
    PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
    Spacing.small
}
