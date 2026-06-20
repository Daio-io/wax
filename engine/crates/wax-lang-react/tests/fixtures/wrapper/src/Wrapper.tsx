import { Button, Tier } from "@acme/design-system";

export function DiscoverScreen() {
  return (
    <Tier>
      <Button />
      <EpisodeCard />
    </Tier>
  );
}

export function EpisodeCard() {
  return <Button />;
}
