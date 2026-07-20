package com.example.app

import com.example.ds.PrimaryButton
import com.example.ds.TextField

@Composable
fun LocalCard(content: @Composable () -> Unit) {
    PrimaryButton(onClick = {})
    content()
}

@Composable
fun SampleScreen() {
    PrimaryButton(onClick = {})
    TextField(value = "", onValueChange = {})
    val primary = Theme.colors.primary
    val ordinary = 200
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Box(
            Modifier
                .padding(4.dp)
                .width(200.dp)
                .height(40.dp)
                .size(4.dp)
                .background(Color(0xFF336699))
                .clip(RoundedCornerShape(4.dp))
                .shadow(4.dp)
        )
        Text(text = "Hello", style = TextStyle(fontSize = 4.sp))
    }
}

@Preview
@Composable
fun SamplePreview() {
    Box(Modifier.padding(99.dp).background(Color(0xFF000000)))
}
