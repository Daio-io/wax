/** Props for `AcmeButton`. */
export interface AcmeButtonProps {
  /** Visible label. Keep it to a short verb phrase such as "Save". */
  label: string;
  /** Called when the user activates the button. */
  onClick: () => void;
  /** Visual emphasis for the action. */
  variant?: 'primary' | 'secondary';
  /** When true, the button ignores input and renders as disabled. */
  disabled?: boolean;
}

/**
 * Triggers the main action on a screen or dialog.
 *
 * Use one primary `AcmeButton` per surface and pair it with a secondary
 * variant for alternative actions.
 *
 * @guidance Use at most one primary button per surface.
 * @guidance Prefer verb labels such as "Save" over "OK".
 * @example
 * <AcmeButton label="Save" onClick={save} />
 */
export function AcmeButton({
  label,
  onClick,
  variant = 'primary',
  disabled = false,
}: AcmeButtonProps) {
  return (
    <button type="button" data-variant={variant} disabled={disabled} onClick={onClick}>
      {label}
    </button>
  );
}
