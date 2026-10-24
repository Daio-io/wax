/**
 * A button used for important actions.
 *
 * @deprecated Use ActionButton instead.
 * @example
 * <Button tone="quiet" />
 */
export interface ButtonProps {
  /** Visual emphasis. */
  tone?: "strong" | "quiet";
  /** Prevents interaction. */
  disabled: boolean;
}

/**
 * A button used for important actions.
 *
 * @deprecated Use ActionButton instead.
 * @example
 * <Button tone="quiet" />
 * @guidance Prefer touch-first controls.
 * @guidance Keep labels concise.
 */
export function RichButton({ tone = "strong", disabled }: ButtonProps) {
  return <button data-tone={tone} disabled={disabled} />;
}
