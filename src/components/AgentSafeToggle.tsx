const SHIELD_ICON = (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.8"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M12 3l7 3v6c0 5-3.2 8.6-7 10-3.8-1.4-7-5-7-10V6l7-3Z" />
  </svg>
);

interface AgentSafeToggleProps {
  safeMode: boolean;
  onToggle: () => void;
  disabled?: boolean;
}

export function AgentSafeToggle({
  safeMode,
  onToggle,
  disabled = false,
}: AgentSafeToggleProps) {
  return (
    <button
      type="button"
      onClick={onToggle}
      disabled={disabled}
      aria-label={safeMode ? 'Safe mode on' : 'Safe mode off'}
      aria-pressed={safeMode}
      title={
        safeMode
          ? 'Safe mode on: non-destructive agent actions only'
          : 'Safe mode off: file writes and stronger agent actions will be allowed'
      }
      className={`shrink-0 flex h-7 w-7 items-center justify-center rounded-lg transition-colors duration-150 disabled:cursor-default disabled:opacity-40 ${
        safeMode
          ? 'text-emerald-300 hover:bg-emerald-400/12'
          : 'text-amber-300 hover:bg-amber-400/12'
      }`}
    >
      {SHIELD_ICON}
    </button>
  );
}
