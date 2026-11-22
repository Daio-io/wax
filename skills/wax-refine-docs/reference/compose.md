# Compose: KDoc that Wax extracts

Golden example: [`examples/compose/AcmeButton.kt`](../examples/compose/AcmeButton.kt)
and the registry details Wax extracts from it in
[`AcmeButton.expected.json`](../examples/compose/AcmeButton.expected.json).

## Which functions are registered

Wax registers top-level, public `@Composable` functions whose names start with
an uppercase letter. It skips `@Preview` functions, `internal`/`private`
functions, functions nested in classes or objects, and scaffolding names that
start with `Provide` or end with `Effect`. Do not spend time documenting
skipped functions for Wax.

## KDoc shape

Put a `/** ... */` block directly above the function. Annotations such as
`@Composable`, `@OptIn`, or `@Deprecated` and `//` comments may sit between the
KDoc and `fun`.

```kotlin
/**
 * Triggers the main action on a screen or dialog.
 *
 * Use one primary [AcmeButton] per surface and pair it with a secondary
 * variant for alternative actions.
 *
 * ```kotlin
 * AcmeButton(text = "Save", onClick = { save() })
 * ```
 *
 * @param text Visible label. Keep it to a short verb phrase such as "Save".
 * @param enabled When false, the button ignores input and renders as disabled.
 * @guidance Use at most one primary button per surface.
 */
```

| KDoc | Registry fact | Notes |
|------|---------------|-------|
| Body text before the tags | `documentation` | Blank lines are dropped and the remaining lines are joined with newlines. |
| `@param name text` | parameter `description` | Lines after it continue the description until the next tag. The first `@param` for a name wins. |
| `@guidance text` | `best_practices[]` | **One line each.** A leading `- ` is stripped. A continuation line becomes body text. |
| Fenced block (three backticks) | example attachment (`kotlin`) | All fenced blocks are concatenated. Blank lines inside the fence are dropped. |
| `@deprecated text` | `deprecation.message` | Used only when there is no `@Deprecated` annotation. |

Every other tag (`@return`, `@see`, `@sample`, `@throws`, ...) is ignored, but
**lines that continue it are read as body text**. Keep those tags on one line
and put all body text before the tags. `@sample` is not resolved, so use a
fenced block for the example.

## Signature facts

Wax reads the parameter list directly:

- `default` comes from the default argument text and `required` is true when
  there is no default.
- `type_kind`:
  - `string`: `String`, `Char`
  - `number`: `Int`, `Long`, `Float`, `Double`, unsigned types, and so on
  - `boolean`: `Boolean`
  - `function`: function types such as `() -> Unit` or `@Composable () -> Unit`
  - `collection`: `List`, `Set`, `Map`, and arrays
  - `object`: other simple, non-generic names
- `enum` with `allowed_values` applies only when the type is an `enum class`
  declared **at top level in the same file**.
- An imported type records `package` (for example `Modifier` gets
  `androidx.compose.ui`) and no `type_kind`. A dotted type such as
  `Arrangement.Horizontal` gets no `type_kind`.

## Structural fixes (each needs approval)

| Symptom | Fix that keeps the API | Notes |
|---------|-------------------------------|-------|
| Enum parameter shows `object` or no `allowed_values` | Move the enum into the component's file at top level, in the same package | The import path stays the same when the package does not change. |
| Enum nested in an object (`AcmeButton.Variant`) | Leave it, or add a same-file top-level enum only if the maintainer wants that API | Do not change public type names for Wax alone. |
| Deprecated component has no replacement | Use `@Deprecated("...", ReplaceWith("NewName(...)"))` | `ReplaceWith` becomes `deprecation.replaced_by`. |

Generic or dotted parameter types are fine to keep. Describe them well in
`@param` instead.
