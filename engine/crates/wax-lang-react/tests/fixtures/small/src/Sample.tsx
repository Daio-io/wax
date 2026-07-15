import { PrimaryButton, TextField } from "@acme/design-system";

export const LocalCard = ({ children }: { children: () => React.ReactNode }) => (
  <>
    <PrimaryButton />
    {children()}
  </>
);

export function SampleScreen() {
  const color = theme.colors.primary;
  return (
    <>
      <PrimaryButton />
      <TextField />
      <div style={{ color: "#336699", padding: 8, borderRadius: 4 }}>{color}</div>
    </>
  );
}
