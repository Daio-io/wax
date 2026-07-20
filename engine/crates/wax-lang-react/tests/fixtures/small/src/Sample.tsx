import { PrimaryButton, TextField } from "@acme/design-system";

export const LocalCard = ({ children }: { children: () => React.ReactNode }) => (
  <>
    <PrimaryButton />
    {children()}
  </>
);

export const SampleScreen = () => {
  const color = theme.colors.primary;
  const ordinary = 200;
  return (
    <>
      <PrimaryButton />
      <TextField />
      <div
        style={{
          color: "#336699",
          padding: "4px",
          marginTop: "4px",
          gap: "4px",
          width: 200,
          height: 40,
          fontSize: "4px",
          borderRadius: "4px",
          boxShadow: "0 1px 2px #000",
        }}
      >
        {color}
        {ordinary}
      </div>
    </>
  );
};
