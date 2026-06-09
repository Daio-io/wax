import { PrimaryButton, TextField } from "@acme/design-system";

export const LocalCard = ({ children }: { children: () => React.ReactNode }) => (
  <>
    <PrimaryButton />
    {children()}
  </>
);

export const SampleScreen = () => (
  <>
    <PrimaryButton />
    <TextField />
  </>
);
