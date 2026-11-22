# Swift: doc comments that Wax extracts

Golden example: [`examples/swift/AcmeButton.swift`](../examples/swift/AcmeButton.swift)
and the registry details Wax extracts from it in
[`AcmeButton.expected.json`](../examples/swift/AcmeButton.expected.json).

## Which declarations are registered

Wax registers top-level, non-private declarations with uppercase names that
are either:

- a `struct` conforming to `View` with a `body`, or
- a function returning `some View`.

The `package` is the module name from `Sources/<Module>/`.

## Doc comment shape

Use `///` lines (or `/** ... */`) **directly above** the declaration. Attribute
lines such as `@MainActor` or `@available` may sit in between. **A blank line
detaches the comment.**

```swift
/// Triggers the main action on a screen or sheet.
///
/// Use one primary ``AcmeButton`` per surface and pair it with a secondary
/// variant for alternative actions.
///
/// - Guidance: Use at most one primary button per surface.
///
/// Example:
/// ```swift
/// AcmeButton("Save") { save() }
/// ```
public struct AcmeButton: View {
```

| Markup | Registry fact | Notes |
|--------|---------------|-------|
| Body text | `documentation` | Blank lines are dropped. Callouts that Wax does not read, such as `- Note:` or `- Returns:`, stay in the text. |
| `- Parameter name: text` or a `- Parameters:` list of `- name: text` | parameter `description` | Use the **internal** parameter name (`title` in `_ title: String`). Indented continuation lines are joined. |
| `- Guidance: text` | `best_practices[]` | Indented continuation lines are joined. |
| `Example:` or `- Example:` and then a fenced block | example attachment (`swift`) | Everything after the marker is read as the example until the next Parameter or Guidance marker, so **put the example last**. |
| `- Deprecated: text` | `deprecation.message` | `@available(*, deprecated, message: "...")` takes precedence. |

## Signature facts and the `public init` rule

For a `View` struct, Wax reads parameters from the **first `public init`
declared in the struct body**. The memberwise initializer is synthesized by the
compiler, so Wax cannot see it. Without an explicit `public init`, the
component reports a `signature.parameters` field gap and has no parameters.

Parameter docs may live on the struct or on the `init`. When both document a
parameter, the struct's description wins. Put the summary on the struct and
the parameter list on the `init`, as in the golden example.

- `default` comes from the default argument (`variant: AcmeButtonVariant = .primary`).
- `type_kind`:
  - `string`: `String`, `Character`
  - `number`: `Int`, sized integer types, `Float`, `Double`
  - `boolean`: `Bool`
  - `function`: closures (including `@escaping`)
  - `collection`: arrays, dictionaries, sets, and tuples
  - `object`: other simple names
- `enum` with `allowed_values` applies only when the type is an `enum`
  declared **at top level in the same file**. A nested enum referenced by a
  simple name (`Variant` declared inside the struct) is classified as `object`.
  Dotted (`Self.Variant`) and custom generic types get no `type_kind`.

## Structural fixes (each needs approval)

| Symptom | Fix that keeps the API | Notes |
|---------|-------------------------------|-------|
| `signature.parameters` gap on a View struct | Add an explicit `public init` with the same labels, order, and defaults as the memberwise initializer callers use today | Keep stored properties and behavior unchanged. Check that existing call sites still compile. |
| Enum parameter has no `allowed_values` | Declare the enum at top level in the component's file | Moving a nested type changes its public name. Only do it if the maintainer agrees. |
| Deprecated view has no message | Use `@available(*, deprecated, message: "Use NewView instead")` | |
