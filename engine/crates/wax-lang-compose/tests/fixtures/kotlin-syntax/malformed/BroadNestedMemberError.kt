import androidx.compose.runtime.Composable

object Screens {
    class Nested {
        @Composable
        fun BeforeNestedGap() {
            PrimaryButton(onClick = {})
        }

        fun BrokenNested() = ()

        @Composable
        fun AfterNestedGap() {
            PrimaryButton(onClick = {})
        }
    }
}
