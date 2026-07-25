import androidx.compose.runtime.Composable

object Screens {
    @Composable
    fun BeforeMemberGap() {
        PrimaryButton(onClick = {})
    }

    fun BrokenMember() = ()

    @Composable
    fun AfterMemberGap() {
        PrimaryButton(onClick = {})
    }
}
