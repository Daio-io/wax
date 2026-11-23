# React: JSDoc/TSDoc that Wax extracts

Golden example: [`examples/react/AcmeButton.tsx`](../examples/react/AcmeButton.tsx)
and the registry details Wax extracts from it in
[`AcmeButton.expected.json`](../examples/react/AcmeButton.expected.json).

## Which exports are registered

Wax registers exported function components: `export function`,
`export const X = (...) =>`, named re-exports of local functions, and
`memo`/`forwardRef` wrappers around them. The `package` is the `name` from the
nearest `package.json` above the discovery root.

## Component JSDoc shape

Put a `/** ... */` block directly before the exported declaration.

````tsx
/**
 * Triggers the main action on a screen or dialog.
 *
 * Use one primary `AcmeButton` per surface and pair it with a secondary
 * variant for alternative actions.
 *
 * @guidance Use at most one primary
 *   button per surface.
 * @example
 * ```tsx
 * <AcmeButton label="Save" onClick={save} />
 * ```
 */
export function AcmeButton({ label, onClick, variant = 'primary' }: AcmeButtonProps) {
````

| JSDoc | Registry fact | Notes |
|-------|---------------|-------|
| Body text | `documentation` | Blank lines are dropped. |
| `@guidance text` | `best_practices[]` | A leading `- ` is stripped. Consecutive lines in the block are joined with spaces. |
| `@example` and the lines after it | example attachment | The example runs until the next tag outside a fence. Wax trims leading whitespace, strips fence lines, and concatenates multiple examples. |
| `@deprecated text` | `deprecation.message` | |

Wax skips other tags (`@param`, `@remarks`, `@see`, `@returns`, ...) and their
continuation lines. A new tag, blank line, or fence ends a guidance or ignored
tag block. Write remarks as plain body text to include them in `documentation`.

## Prop docs live on the props type

Wax does **not** read `@param props.label` on the component. It reads the
JSDoc on each member of the props type:

```tsx
export interface AcmeButtonProps {
  /** Visible label. Keep it to a short verb phrase such as "Save". */
  label: string;
  /** Visual emphasis for the action. */
  variant?: 'primary' | 'secondary';
}
```

Wax reads members from the type annotation on the component's first parameter
when it is:

- a local `interface` **without `extends`**,
- a local `type X = { ... }` object literal, or
- an inline object type.

Other shapes lose structured facts:

| Props shape | Result |
|-------------|--------|
| `interface X extends Y` | Members and descriptions are kept, but `type_kind`/`allowed_values` are cleared and an inherits field gap is recorded. |
| Intersection alias (`type X = A & B`) | No parameters, and a field gap is recorded. |
| Imported props type | One opaque parameter with `package` and no member facts. |
| No type annotation | No parameters, and a field gap is recorded. |

Per-prop facts:

- `required` is false for `?` members.
- `default` comes from **destructuring defaults** (`variant = 'primary'`).
  `defaultProps` and `@default` tags are not read.
- `type_kind`:
  - `string`, `number`, `boolean`: the matching keyword types
  - `function`: function types
  - `collection`: arrays and tuples
  - `object`: `object` and inline object literals
  - `enum` with `allowed_values` is extracted from inline literal unions, from
    top-level literal-union aliases in the same file, and from top-level
    TypeScript enums in the same file. Aliases and enums can be exported.
- String union members are decoded, so `'primary' | "secondary"` yields
  `["primary", "secondary"]`. Numeric, boolean, and bigint members keep their
  source spelling. TypeScript enums contribute member names, so
  `enum Variant { Primary = 'primary', Secondary = 'secondary' }` yields
  `["Primary", "Secondary"]`.
- Imported types, unions with non-literal members such as `undefined`, and
  unions containing template literal types with substitutions or empty
  string values have no enum facts. Use `?` for optional props.

## Structural fixes (each needs approval)

| Symptom | Fix that keeps the API | Notes |
|---------|-------------------------------|-------|
| Defaults are set inside the body or through `defaultProps` | Move them into the destructured parameter defaults | Behavior stays the same when the value is used only through the destructured name. |
| Props `extends` native element attributes | Usually keep it and accept the gap | Descriptions on the component's own members are still extracted. Do not reshape public props types for Wax alone. |
| Docs written as component `@param` tags | Move each description onto the matching props member | Doc-only change. |
