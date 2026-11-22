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

```tsx
/**
 * Triggers the main action on a screen or dialog.
 *
 * Use one primary `AcmeButton` per surface and pair it with a secondary
 * variant for alternative actions.
 *
 * @guidance Use at most one primary button per surface.
 * @example
 * <AcmeButton label="Save" onClick={save} />
 */
export function AcmeButton({ label, onClick, variant = 'primary' }: AcmeButtonProps) {
```

| JSDoc | Registry fact | Notes |
|-------|---------------|-------|
| Body text before the tags | `documentation` | Blank lines are dropped. |
| `@guidance text` | `best_practices[]` | **One line each.** A leading `- ` is stripped. |
| `@example` and the lines after it | example attachment | Read verbatim until the next tag. Leading whitespace is trimmed. **Leave out code fences**, because fence lines would become part of the example. Several `@example` blocks are concatenated. |
| `@deprecated text` | `deprecation.message` | |

Every other tag (`@param`, `@remarks`, `@see`, `@returns`, ...) is ignored,
including any text on the tag's own line. **Continuation lines after a tag
other than `@example` are read as body text.** Put all body text before the
tags, write a TSDoc `@remarks` section as plain body text, and keep the other
tags on one line.

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
- `enum` with `allowed_values` applies only to an **inline** union of literals
  (`'primary' | 'secondary'`). Values keep their quotes as written. A named
  alias (`variant?: ButtonVariant`) or a union that includes non-literals such
  as `| undefined` gets no `type_kind`. Use `?` for optional props instead.

## Structural fixes (each needs approval)

| Symptom | Fix that keeps the API | Notes |
|---------|-------------------------------|-------|
| `variant` has no `allowed_values` | Inline the literal union in the prop, and keep the exported alias for consumers if one exists | The resulting type stays the same. |
| Defaults are set inside the body or through `defaultProps` | Move them into the destructured parameter defaults | Behavior stays the same when the value is used only through the destructured name. |
| Props `extends` native element attributes | Usually keep it and accept the gap | Descriptions on the component's own members are still extracted. Do not reshape public props types for Wax alone. |
| Docs written as component `@param` tags | Move each description onto the matching props member | Doc-only change. |
