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

class Controller {
    @Composable
    fun BeforeMemberSyntax() {
        PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
        Spacing.small
    }

    fun suspend() = Unit

    fun <T> suspend(value: T): T = value

    fun ItemScope.suspend() = Unit

    context(itemScope: ItemScope)
    @Composable
    fun ContextualMember() {
        itemScope.hashCode()
        PrimaryButton(onClick = {})
    }

    @Composable
    fun AfterMemberSyntax() {
        PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
        Spacing.small
    }
}

object ObjectController {
    context(itemScope: ItemScope)
    @Composable
    fun ContextualObjectMember() {
        itemScope.hashCode()
        PrimaryButton(onClick = {})
    }

    @Composable
    fun AfterObjectMemberSyntax() {
        PrimaryButton(onClick = {}, modifier = Modifier.padding(7.dp))
        Spacing.small
    }
}
