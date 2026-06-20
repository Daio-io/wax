package com.example.discover

@Composable
fun DiscoverScreen() {
    EpisodeCard()
    EpisodeCard()
}

@Composable
fun EpisodeCard() {
    Tier { BodyText("title") }
}
