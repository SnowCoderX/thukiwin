import { motion, AnimatePresence } from 'framer-motion';
import type React from 'react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { formatQuotedText } from '../utils/formatQuote';
import { quote } from '../config';
import { ImageThumbnails } from '../components/ImageThumbnails';
import { CommandSuggestion } from '../components/CommandSuggestion';
import { ModelSelector } from '../components/ModelSelector';
import { AgentSafeToggle } from '../components/AgentSafeToggle';
import { Tooltip } from '../components/Tooltip';
import type { AttachedImage } from '../types/image';
import { MAX_IMAGE_SIZE_BYTES } from '../types/image';
import { COMMANDS } from '../config/commands';

const ARROW_UP_ICON = (
  <svg
    width="16"
    height="16"
    viewBox="0 0 16 16"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <path
      d="M8 13V3M8 3L3 8M8 3L13 8"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);

const STOP_ICON = (
  <svg
    width="16"
    height="16"
    viewBox="0 0 16 16"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <rect x="3" y="3" width="10" height="10" rx="2" fill="currentColor" />
  </svg>
);

const BORDER_TRACE_RING = (
  <svg
    className="stop-ring-svg"
    viewBox="0 0 40 40"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <rect
      className="stop-trace-tail"
      x="1"
      y="1"
      width="38"
      height="38"
      rx="13"
      pathLength="100"
    />
    <rect
      className="stop-trace-mid"
      x="1"
      y="1"
      width="38"
      height="38"
      rx="13"
      pathLength="100"
    />
    <rect
      className="stop-trace-head"
      x="1"
      y="1"
      width="38"
      height="38"
      rx="13"
      pathLength="100"
    />
  </svg>
);

/**
 * Кольцо обратного отсчёта до авто-отправки голосовой команды (после
 * "туки"). `fraction` растёт от 0 до 1 по мере тишины после речи — когда
 * доходит до 1, происходит отправка. Используем тот же приём pathLength=100,
 * что и BORDER_TRACE_RING, но как один убывающий/растущий strokeDashoffset,
 * а не готовую CSS-анимацию — потому что здесь прогресс управляется живым
 * значением из бэкенда, а не фиксированной длительностью.
 */
function AutoSendRing({ fraction }: { fraction: number }) {
  return (
    <svg
      viewBox="0 0 40 40"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
      className="absolute inset-0 w-full h-full pointer-events-none"
    >
      <rect
        x="1"
        y="1"
        width="38"
        height="38"
        rx="13"
        pathLength="100"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeDasharray="100"
        strokeDashoffset={100 - fraction * 100}
        style={{ transition: 'stroke-dashoffset 100ms linear' }}
      />
    </svg>
  );
}

const HISTORY_ICON = (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <circle
      cx="12"
      cy="12"
      r="10"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
    />
    <polyline
      points="12 6 12 12 16 14"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);

const CAMERA_ICON = (
  <svg
    width="14"
    height="14"
    viewBox="0 0 16 16"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <path
      d="M2 6 L2 2 L6 2 M10 2 L14 2 L14 6 M2 10 L2 14 L6 14 M10 14 L14 14 L14 10"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);

const MIC_ICON = (
  <svg
    width="14"
    height="14"
    viewBox="0 0 16 16"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <rect x="6" y="1" width="4" height="8" rx="2" fill="currentColor" />
    <path
      d="M3 7v1a5 5 0 0 0 10 0V7M8 13v2"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
    />
  </svg>
);

export function renderHighlightedText(text: string): React.ReactNode {
  const parts: React.ReactNode[] = [];
  let remaining = text;
  const highlighted = new Set<string>();

  while (remaining.length > 0) {
    let earliest = -1;
    let matchedTrigger = '';
    for (const cmd of COMMANDS) {
      if (highlighted.has(cmd.trigger)) continue;
      const idx = remaining.indexOf(cmd.trigger);
      if (idx !== -1 && (earliest === -1 || idx < earliest)) {
        const before = idx === 0 || remaining[idx - 1] === ' ';
        const after =
          idx + cmd.trigger.length >= remaining.length ||
          remaining[idx + cmd.trigger.length] === ' ';
        if (before && after) {
          earliest = idx;
          matchedTrigger = cmd.trigger;
        }
      }
    }

    if (earliest === -1) {
      parts.push(<span key={parts.length}>{remaining}</span>);
      break;
    }

    if (earliest > 0) {
      parts.push(
        <span key={parts.length}>{remaining.slice(0, earliest)}</span>,
      );
    }
    parts.push(
      <span key={parts.length} className="text-violet-400">
        {matchedTrigger}
      </span>,
    );
    highlighted.add(matchedTrigger);
    remaining = remaining.slice(earliest + matchedTrigger.length);
  }

  return <>{parts}</>;
}

export const MAX_IMAGES = 3;

interface AskBarViewProps {
  query: string;
  setQuery: React.Dispatch<React.SetStateAction<string>>;
  isChatMode: boolean;
  isGenerating: boolean;
  isSubmitPending?: boolean;
  onSubmit: () => void;
  onCancel: () => void;
  inputRef: React.RefObject<HTMLTextAreaElement | null>;
  selectedText?: string;
  onHistoryOpen?: () => void;
  attachedImages: AttachedImage[];
  onImagesAttached: (files: File[]) => void;
  onImageRemove: (id: string) => void;
  onImagePreview: (id: string) => void;
  onScreenshot: () => void;
  onVoiceToggle?: () => void;
  voiceStatus?: 'idle' | 'recording' | 'finishing' | 'error';
  voiceVolume?: number;
  /** 0..1 — доля тишины до авто-отправки голосовой команды (после "туки"). */
  autoSendFraction?: number;
  availableModels: string[];
  activeModel: string;
  onModelChange: (model: string) => void;
  safeMode: boolean;
  onSafeModeToggle: () => void;
  agentEnabled: boolean;
  onAgentEnabledToggle: () => void;
  wakeWordEnabled?: boolean;
  onWakeWordToggle?: () => void;
  headphonesMode?: boolean;
  onHeadphonesModeToggle?: () => void;
  isDragOver?: 'normal' | 'max';
  /** Name of the active context profile (shown in the toolbar). */
  activeProfileName?: string;
  /** Called when the user clicks the profile badge to open the manager. */
  onProfileClick?: () => void;
}

export function AskBarView({
  query,
  setQuery,
  isChatMode,
  isGenerating,
  isSubmitPending = false,
  onSubmit,
  onCancel,
  inputRef,
  selectedText,
  onHistoryOpen,
  attachedImages,
  onImagesAttached,
  onImageRemove,
  onImagePreview,
  onScreenshot,
  onVoiceToggle = () => {},
  voiceStatus = 'idle',
  voiceVolume = 0,
  autoSendFraction = 0,
  availableModels,
  activeModel,
  onModelChange,
  safeMode,
  onSafeModeToggle,
  agentEnabled,
  onAgentEnabledToggle,
  wakeWordEnabled = true,
  onWakeWordToggle = () => {},
  headphonesMode = false,
  onHeadphonesModeToggle = () => {},
  isDragOver,
  activeProfileName,
  onProfileClick,
}: AskBarViewProps) {
  const mirrorRef = useRef<HTMLDivElement>(null);
  const isBusy = isGenerating || isSubmitPending;
  const canSubmit =
    (query.trim().length > 0 || attachedImages.length > 0) && !isBusy;
  const isAtMaxImages = attachedImages.length >= MAX_IMAGES;
  const [pasteMaxError, setPasteMaxError] = useState(false);

  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 144)}px`;
  }, [query, inputRef]);

  useEffect(() => {
    if (!pasteMaxError) return;
    const timer = setTimeout(() => setPasteMaxError(false), 2000);
    return () => clearTimeout(timer);
  }, [pasteMaxError]);

  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const [dismissedQuery, setDismissedQuery] = useState('');

  const rawQuery = query.trimStart();
  const lastSlashWord = useMemo(() => {
    const match = rawQuery.match(/(?:^|\s)(\/\S*)$/);
    return match ? match[1] : '';
  }, [rawQuery]);

  const showSuggestions =
    !isBusy && lastSlashWord.length > 0 && lastSlashWord !== dismissedQuery;
  const commandPrefix = showSuggestions ? lastSlashWord : '';

  const usedCommands = useMemo(() => {
    const textBeforeSlash = rawQuery.slice(
      0,
      rawQuery.length - lastSlashWord.length,
    );
    return new Set(
      COMMANDS.filter((cmd) => {
        const idx = textBeforeSlash.indexOf(cmd.trigger);
        if (idx === -1) return false;
        const before = idx === 0 || textBeforeSlash[idx - 1] === ' ';
        const after =
          idx + cmd.trigger.length >= textBeforeSlash.length ||
          textBeforeSlash[idx + cmd.trigger.length] === ' ';
        return before && after;
      }).map((cmd) => cmd.trigger),
    );
  }, [rawQuery, lastSlashWord]);

  const filteredCommands = useMemo(
    () =>
      showSuggestions
        ? COMMANDS.filter(
            (cmd) =>
              cmd.trigger.startsWith(commandPrefix) &&
              !usedCommands.has(cmd.trigger),
          )
        : [],
    [showSuggestions, commandPrefix, usedCommands],
  );

  useEffect(() => {
    setHighlightedIndex(0);
  }, [commandPrefix]);

  const handleCommandSelect = useCallback(
    (trigger: string) => {
      setDismissedQuery('');
      setHighlightedIndex(0);
      const beforeSlash = rawQuery.slice(
        0,
        rawQuery.length - lastSlashWord.length,
      );
      setQuery(beforeSlash + trigger + ' ');
    },
    [setQuery, rawQuery, lastSlashWord],
  );

  const handleTextareaChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const newValue = e.target.value;
      setDismissedQuery('');
      setQuery(newValue);
      const el = e.target;
      el.style.height = 'auto';
      el.style.height = `${Math.min(el.scrollHeight, 144)}px`;
    },
    [setQuery],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (showSuggestions) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          if (filteredCommands.length > 0) {
            setHighlightedIndex((i) => (i + 1) % filteredCommands.length);
          }
          return;
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          if (filteredCommands.length > 0) {
            setHighlightedIndex(
              (i) =>
                (i - 1 + filteredCommands.length) % filteredCommands.length,
            );
          }
          return;
        }
        if (e.key === 'Tab') {
          e.preventDefault();
          if (filteredCommands.length > 0) {
            const idx = Math.min(highlightedIndex, filteredCommands.length - 1);
            handleCommandSelect(filteredCommands[idx].trigger);
          }
          return;
        }
        if (e.key === 'Enter' && !e.shiftKey) {
          if (
            filteredCommands.length > 0 &&
            highlightedIndex < filteredCommands.length
          ) {
            const selectedTrigger = filteredCommands[highlightedIndex].trigger;
            if (lastSlashWord !== selectedTrigger) {
              e.preventDefault();
              handleCommandSelect(selectedTrigger);
              return;
            }
          }
        }
        if (e.key === 'Escape') {
          e.preventDefault();
          setDismissedQuery(lastSlashWord);
          return;
        }
      }

      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        onSubmit();
      }
    },
    [
      showSuggestions,
      filteredCommands,
      highlightedIndex,
      handleCommandSelect,
      lastSlashWord,
      onSubmit,
    ],
  );

  const handleTextareaScroll = useCallback(() => {
    if (!mirrorRef.current || !inputRef.current) return;
    mirrorRef.current.scrollTop = inputRef.current.scrollTop;
  }, [inputRef]);

  const handlePaste = useCallback(
    (e: React.ClipboardEvent) => {
      const items = e.clipboardData?.items;
      if (!items || isBusy) return;

      const remaining = MAX_IMAGES - attachedImages.length;
      if (remaining <= 0) {
        const hasImageItem = Array.from(items).some((item) =>
          item.type.startsWith('image/'),
        );
        if (hasImageItem) setPasteMaxError(true);
        return;
      }

      const imageFiles: File[] = [];
      for (let i = 0; i < items.length && imageFiles.length < remaining; i++) {
        if (items[i].type.startsWith('image/')) {
          const file = items[i].getAsFile();
          if (file && file.size <= MAX_IMAGE_SIZE_BYTES) {
            imageFiles.push(file);
          }
        }
      }

      if (imageFiles.length === 0) return;
      e.preventDefault();
      onImagesAttached(imageFiles);
    },
    [isBusy, attachedImages.length, onImagesAttached],
  );

  const showMaxLabel = isDragOver === 'max' || (pasteMaxError && !isDragOver);
  const ringClass =
    isDragOver === 'max'
      ? 'ring-2 ring-red-500/60 ring-inset rounded-lg'
      : isDragOver === 'normal'
        ? 'ring-2 ring-primary/40 ring-inset rounded-lg'
        : '';

  return (
    <div className={`flex flex-col w-full shrink-0 ${ringClass}`}>
      {selectedText && (
        <div className="px-4 pt-2 pb-0">
          <p className="italic text-xs text-text-secondary select-text whitespace-pre-wrap">
            &ldquo;
            {formatQuotedText(
              selectedText,
              quote.maxDisplayLines,
              quote.maxDisplayChars,
            )}
            &rdquo;
          </p>
        </div>
      )}
      {showMaxLabel && (
        <p className="px-4 pt-2 pb-0 text-xs text-red-400">Max 3 images</p>
      )}
      {attachedImages.length > 0 && (
        <div className="px-4 pt-2 pb-0">
          <ImageThumbnails
            items={attachedImages.map((img) => ({
              id: img.id,
              src: img.blobUrl,
              loading: img.filePath === null,
            }))}
            onPreview={onImagePreview}
            onRemove={onImageRemove}
            size={56}
          />
        </div>
      )}
      <AnimatePresence>
        {showSuggestions && (
          <motion.div
            key="command-suggestion"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{
              height: { duration: 0.2, ease: [0.16, 1, 0.3, 1] },
              opacity: { duration: 0.15 },
            }}
            style={{ overflow: 'hidden' }}
          >
            <CommandSuggestion
              commands={filteredCommands}
              highlightedIndex={highlightedIndex}
              onSelect={handleCommandSelect}
            />
          </motion.div>
        )}
      </AnimatePresence>
      <div className="relative">
        <div className="flex items-center w-full px-3 py-2.5 gap-2">
          <img
            src="/thuki-logo.png"
            alt="Thuki"
            className={`shrink-0 transition-all duration-300 ease-out ${
              isChatMode ? 'w-6 h-6 rounded-lg' : 'w-10 h-10 rounded-lg'
            }`}
            draggable={false}
          />

          {!isChatMode && onHistoryOpen && (
            <button
              type="button"
              onClick={onHistoryOpen}
              aria-label="Open history"
              className="shrink-0 w-7 h-7 flex items-center justify-center rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/8 transition-colors duration-150 cursor-pointer outline-none"
            >
              {HISTORY_ICON}
            </button>
          )}

          <ModelSelector
            models={availableModels}
            activeModel={activeModel}
            onModelChange={onModelChange}
            disabled={isBusy}
          />

          {/* ── PROFILE: active profile badge ── */}
          {onProfileClick && (
            <Tooltip label={`Profile: ${activeProfileName ?? 'General'}`}>
              <button
                type="button"
                onClick={onProfileClick}
                className="shrink-0 w-7 h-7 flex items-center justify-center rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/8 transition-colors duration-150 cursor-pointer outline-none"
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
                  <path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" />
                  <circle cx="12" cy="7" r="4" />
                </svg>
              </button>
            </Tooltip>
          )}
          {/* ── END PROFILE ── */}

          <AgentSafeToggle
            safeMode={safeMode}
            onToggle={onSafeModeToggle}
            agentEnabled={agentEnabled}
            onAgentEnabledToggle={onAgentEnabledToggle}
            wakeWordEnabled={wakeWordEnabled}
            onWakeWordToggle={onWakeWordToggle}
            headphonesMode={headphonesMode}
            onHeadphonesModeToggle={onHeadphonesModeToggle}
            disabled={isBusy}
          />

          <div className="relative flex-1 min-w-0">
            <div
              ref={mirrorRef}
              data-testid="askbar-mirror"
              aria-hidden="true"
              className="absolute inset-0 pointer-events-none bg-transparent text-text-primary text-sm py-2 px-1 leading-relaxed whitespace-pre-wrap break-words overflow-hidden"
            >
              {renderHighlightedText(query)}
            </div>
            <textarea
              ref={inputRef}
              value={query}
              onChange={handleTextareaChange}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
              onScroll={handleTextareaScroll}
              autoFocus
              rows={1}
              placeholder={isChatMode ? 'Reply...' : 'Ask Thuki anything...'}
              className="relative w-full bg-transparent border-none outline-none text-transparent text-sm placeholder:text-text-secondary py-2 px-1 resize-none leading-relaxed"
              style={{ caretColor: 'var(--color-text-primary)' }}
            />
          </div>

          <Tooltip
            label={
              voiceStatus === 'recording'
                ? 'Stop recording'
                : voiceStatus === 'finishing'
                  ? 'Finishing...'
                  : 'Voice input'
            }
          >
            <div className="relative flex items-center justify-center">
              {voiceStatus === 'recording' && (
                <div
                  className="absolute rounded-full bg-red-500/30 transition-all duration-75 ease-out"
                  style={{
                    width: `${28 + (voiceVolume ?? 0) * 0.35}px`,
                    height: `${28 + (voiceVolume ?? 0) * 0.35}px`,
                  }}
                />
              )}
              <button
                type="button"
                onClick={onVoiceToggle}
                disabled={isBusy || voiceStatus === 'finishing'}
                aria-label="Toggle voice input"
                className={`relative z-10 shrink-0 w-7 h-7 flex items-center justify-center rounded-lg transition-colors duration-150 disabled:opacity-40 disabled:cursor-default cursor-pointer ${
                  voiceStatus === 'recording'
                    ? 'text-red-400 bg-red-500/10'
                    : 'text-text-secondary hover:text-text-primary hover:bg-white/8'
                }`}
              >
                {MIC_ICON}
              </button>
            </div>
          </Tooltip>

          {isAtMaxImages ? (
            <Tooltip label="Maximum 3 images attached">
              <button
                type="button"
                onClick={onScreenshot}
                disabled
                aria-label="Take screenshot"
                className="shrink-0 w-7 h-7 flex items-center justify-center rounded-lg text-text-secondary transition-colors duration-150 disabled:opacity-40 disabled:cursor-default cursor-pointer"
              >
                {CAMERA_ICON}
              </button>
            </Tooltip>
          ) : (
            <Tooltip label="Take a screenshot">
              <button
                type="button"
                onClick={onScreenshot}
                disabled={isBusy}
                aria-label="Take screenshot"
                className="shrink-0 w-7 h-7 flex items-center justify-center rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/8 transition-colors duration-150 disabled:opacity-40 disabled:cursor-default cursor-pointer"
              >
                {CAMERA_ICON}
              </button>
            </Tooltip>
          )}

          <motion.button
            type="button"
            onClick={isBusy ? onCancel : onSubmit}
            disabled={!canSubmit && !isBusy}
            whileHover={canSubmit || isBusy ? { scale: 1.08 } : undefined}
            whileTap={canSubmit || isBusy ? { scale: 0.92 } : undefined}
            className={`relative shrink-0 w-9 h-9 rounded-lg flex items-center justify-center transition-colors duration-200 ${
              isBusy
                ? 'stop-btn-ring bg-red-500/10 text-red-400 cursor-pointer'
                : canSubmit
                  ? 'bg-primary text-neutral cursor-pointer'
                  : 'bg-surface-elevated text-text-secondary cursor-default'
            }`}
            aria-label={isBusy ? 'Stop generating' : 'Send message'}
          >
            {isBusy ? (
              <>
                {BORDER_TRACE_RING}
                {STOP_ICON}
              </>
            ) : (
              <>
                {autoSendFraction > 0 && <AutoSendRing fraction={autoSendFraction} />}
                {ARROW_UP_ICON}
              </>
            )}
          </motion.button>
        </div>
      </div>
    </div>
  );
}