import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Tooltip } from './Tooltip';

interface AgentSafeToggleProps {
  safeMode: boolean;
  onToggle: () => void;
  agentEnabled: boolean;
  onAgentEnabledToggle: () => void;
  wakeWordEnabled?: boolean;
  onWakeWordToggle?: () => void;
  /** Двойной Ctrl — вызов оверлея откуда угодно. Отдельный тумблер от wake-word. */
  globalHotkeyEnabled?: boolean;
  onGlobalHotkeyToggle?: () => void;
  headphonesMode?: boolean;
  onHeadphonesModeToggle?: () => void;
  disabled?: boolean;
}

/**
 * Панель тумблеров ask-бара (safe mode, agent, wake word, Ctrl-хоткей,
 * headphones). Свёрнута в одну кнопку по умолчанию, чтобы не раздувать
 * ширину бара — клик по ней разворачивает панель с иконками.
 */
export function AgentSafeToggle({
  safeMode,
  onToggle,
  agentEnabled,
  onAgentEnabledToggle,
  wakeWordEnabled = true,
  onWakeWordToggle = () => {},
  globalHotkeyEnabled = true,
  onGlobalHotkeyToggle = () => {},
  headphonesMode = false,
  onHeadphonesModeToggle = () => {},
  disabled = false,
}: AgentSafeToggleProps) {
  const [isExpanded, setIsExpanded] = useState(false);

  return (
    <div className="flex items-center rounded-lg overflow-hidden border border-white/10">
      <Tooltip label={isExpanded ? 'Свернуть настройки' : 'Настройки'}>
        <button
          type="button"
          onClick={() => setIsExpanded((prev) => !prev)}
          disabled={disabled}
          aria-expanded={isExpanded}
          aria-label={isExpanded ? 'Свернуть настройки' : 'Развернуть настройки'}
          className="shrink-0 w-7 h-7 flex items-center justify-center transition-colors duration-150 cursor-pointer outline-none text-text-secondary hover:text-text-primary hover:bg-white/8"
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <line x1="4" y1="6" x2="20" y2="6" />
            <line x1="4" y1="12" x2="20" y2="12" />
            <line x1="4" y1="18" x2="20" y2="18" />
            <circle cx="9" cy="6" r="1.5" fill="currentColor" stroke="none" />
            <circle cx="15" cy="12" r="1.5" fill="currentColor" stroke="none" />
            <circle cx="9" cy="18" r="1.5" fill="currentColor" stroke="none" />
          </svg>
          <svg
            width="8"
            height="8"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="3"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
            className={`ml-0.5 transition-transform duration-150 ${isExpanded ? 'rotate-180' : ''}`}
          >
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>
      </Tooltip>

      <AnimatePresence initial={false}>
        {isExpanded && (
          <motion.div
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: 'auto', opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={{ duration: 0.15, ease: [0.16, 1, 0.3, 1] }}
            className="flex items-center overflow-hidden"
          >
            <div className="w-px h-4 bg-white/10 shrink-0" />

            <Tooltip label={safeMode ? 'Safe mode: on' : 'Safe mode: off'}>
              <button
                type="button"
                onClick={onToggle}
                disabled={disabled}
                className={`shrink-0 w-7 h-7 flex items-center justify-center transition-colors duration-150 cursor-pointer outline-none ${
                  safeMode
                    ? 'text-amber-400 bg-amber-500/10'
                    : 'text-text-secondary hover:text-text-primary hover:bg-white/8'
                }`}
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                </svg>
              </button>
            </Tooltip>

            <div className="w-px h-4 bg-white/10 shrink-0" />

            <Tooltip label={agentEnabled ? 'Agent: on' : 'Agent: off'}>
              <button
                type="button"
                onClick={onAgentEnabledToggle}
                disabled={disabled}
                className={`shrink-0 w-7 h-7 flex items-center justify-center transition-colors duration-150 cursor-pointer outline-none ${
                  agentEnabled
                    ? 'text-violet-400 bg-violet-500/10'
                    : 'text-text-secondary hover:text-text-primary hover:bg-white/8'
                }`}
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                  <line x1="8" y1="21" x2="16" y2="21" />
                  <line x1="12" y1="17" x2="12" y2="21" />
                </svg>
              </button>
            </Tooltip>

            <div className="w-px h-4 bg-white/10 shrink-0" />

            <Tooltip label={wakeWordEnabled ? 'Wake word: on' : 'Wake word: off'}>
              <button
                type="button"
                onClick={onWakeWordToggle}
                disabled={disabled}
                aria-label={wakeWordEnabled ? 'Disable wake word' : 'Enable wake word'}
                className={`shrink-0 w-7 h-7 flex items-center justify-center transition-colors duration-150 cursor-pointer outline-none ${
                  wakeWordEnabled
                    ? 'text-emerald-400 bg-emerald-500/10'
                    : 'text-text-secondary hover:text-text-primary hover:bg-white/8'
                }`}
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
                  <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                  <line x1="12" y1="19" x2="12" y2="23" />
                  <line x1="8" y1="23" x2="16" y2="23" />
                </svg>
              </button>
            </Tooltip>

            <div className="w-px h-4 bg-white/10 shrink-0" />

            <Tooltip
              label={
                globalHotkeyEnabled
                  ? 'Двойной Ctrl: вызов включён'
                  : 'Двойной Ctrl: вызов выключен'
              }
            >
              <button
                type="button"
                onClick={onGlobalHotkeyToggle}
                disabled={disabled}
                aria-label={
                  globalHotkeyEnabled ? 'Disable double-Ctrl activation' : 'Enable double-Ctrl activation'
                }
                className={`shrink-0 w-7 h-7 flex items-center justify-center transition-colors duration-150 cursor-pointer outline-none ${
                  globalHotkeyEnabled
                    ? 'text-sky-400 bg-sky-500/10'
                    : 'text-text-secondary hover:text-text-primary hover:bg-white/8'
                }`}
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <rect x="3" y="4" width="18" height="16" rx="2" />
                  <path d="M7 9h3M7 13h3" />
                  <path d="M14 9h3M14 13h3" />
                  <path d="M7 17h10" />
                </svg>
              </button>
            </Tooltip>

            <div className="w-px h-4 bg-white/10 shrink-0" />

            <Tooltip
              label={
                headphonesMode
                  ? 'Headphones: wake word ignores output'
                  : 'Speakers: wake word muted during output'
              }
            >
              <button
                type="button"
                onClick={onHeadphonesModeToggle}
                disabled={disabled}
                className={`shrink-0 w-7 h-7 flex items-center justify-center transition-colors duration-150 cursor-pointer outline-none ${
                  headphonesMode
                    ? 'text-cyan-400 bg-cyan-500/10'
                    : 'text-text-secondary hover:text-text-primary hover:bg-white/8'
                }`}
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
                  <path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3zM3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3z" />
                </svg>
              </button>
            </Tooltip>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
