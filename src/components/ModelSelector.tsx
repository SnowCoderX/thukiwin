import { useMemo } from 'react';

const MODEL_ICON = (
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
    <path d="M12 3 4 7v10l8 4 8-4V7l-8-4Z" />
    <path d="M4 7l8 4 8-4" />
    <path d="M12 11v10" />
  </svg>
);

interface ModelSelectorProps {
  /** All available model names shown in the picker. */
  models: string[];
  /** Currently active model name. */
  activeModel: string;
  /** Called when the user picks a different model. */
  onModelChange: (model: string) => void;
  /** Disables the selector while busy. */
  disabled?: boolean;
}

/**
 * Tiny icon-only wrapper around a native select.
 * The OS renders the dropdown itself, so it won't clip inside the overlay and
 * doesn't require a custom popup container.
 */
export function ModelSelector({
  models,
  activeModel,
  onModelChange,
  disabled = false,
}: ModelSelectorProps) {
  const options = useMemo(() => {
    const seen = new Set<string>();
    const merged = [activeModel, ...models].filter((model) => {
      const trimmed = model.trim();
      if (!trimmed || seen.has(trimmed)) return false;
      seen.add(trimmed);
      return true;
    });

    return merged.length > 0 ? merged : [activeModel];
  }, [activeModel, models]);

  return (
    <div
      className="group relative shrink-0 rounded-lg"
      data-model-selector
      title={activeModel}
    >
      <div className="pointer-events-none flex h-7 w-7 items-center justify-center rounded-lg text-text-secondary transition-colors duration-150 group-hover:bg-white/8 group-hover:text-text-primary">
        {MODEL_ICON}
      </div>

      <label htmlFor="thuki-model-select" className="sr-only">
        Select model
      </label>
      <select
        id="thuki-model-select"
        value={activeModel}
        onChange={(e) => onModelChange(e.target.value)}
        disabled={disabled}
        aria-label="Select model"
        className="absolute inset-0 h-7 w-7 cursor-pointer appearance-none rounded-lg opacity-0 disabled:cursor-default"
        style={{
          colorScheme: 'dark',
          backgroundColor: '#202020',
          color: '#f0f0f2',
        }}
      >
        {options.map((model) => (
          <option
            key={model}
            value={model}
            style={{
              backgroundColor: '#202020',
              color: '#f0f0f2',
            }}
          >
            {model}
          </option>
        ))}
      </select>
    </div>
  );
}
