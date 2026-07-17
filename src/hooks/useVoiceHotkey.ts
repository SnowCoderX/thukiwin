import { useEffect, useRef, useState } from 'react';

const DOUBLE_TAP_WINDOW_MS = 400;
const ARMED_TIMEOUT_MS = 3000;
const MIN_RECORDING_MS = 1000;

export type HotkeyUiState = 'hidden' | 'armed' | 'recording';

interface UseVoiceHotkeyOptions {
  onStart: () => void;
  onStop: () => void;
  onCancel: () => void;
  /** true пока идёт запись, чтобы не запустить вторую поверх текущей. */
  isRecording: boolean;
}

/**
 * Двойное нажатие Ctrl показывает панель голосового ввода ("armed"), после
 * этого удержание Ctrl пишет голос, отпускание останавливает и отдаёт на
 * расшифровку. Если удержание короче секунды, запись отменяется целиком,
 * это защита от случайных Ctrl+C/Ctrl+V и прочих сочетаний, задетых во
 * время "вооружённого" состояния.
 *
 * Любая другая клавиша, нажатая вместе с Ctrl, до старта записи сбрасывает
 * счётчик двойного нажатия, чтобы обычные хоткеи не путались с этим жестом.
 * Слушает window, поэтому работает независимо от того, где сейчас фокус.
 */
export function useVoiceHotkey({ onStart, onStop, onCancel, isRecording }: UseVoiceHotkeyOptions) {
  const [uiState, setUiState] = useState<HotkeyUiState>('hidden');

  const lastCtrlUpAt = useRef(0);
  const armed = useRef(false);
  const armedTimeout = useRef<number | null>(null);
  const recordingStartedAt = useRef(0);
  const comboUsed = useRef(false);
  const ctrlDown = useRef(false);

  useEffect(() => {
    const clearArmedTimeout = () => {
      if (armedTimeout.current !== null) {
        window.clearTimeout(armedTimeout.current);
        armedTimeout.current = null;
      }
    };

    const disarm = () => {
      armed.current = false;
      clearArmedTimeout();
      setUiState((s) => (s === 'armed' ? 'hidden' : s));
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      const isCtrl = e.key === 'Control';

      if (isCtrl && !ctrlDown.current) {
        ctrlDown.current = true;
        comboUsed.current = false;

        if (armed.current && !isRecording) {
          // Третье нажатие подряд после двойного тапа: начинаем запись.
          clearArmedTimeout();
          recordingStartedAt.current = Date.now();
          setUiState('recording');
          onStart();
        }
        return;
      }

      // Любая другая клавиша с зажатым Ctrl - это обычный хоткей пользователя,
      // а не наш жест, аннулируем накопленный двойной тап.
      if (ctrlDown.current && !isCtrl) {
        comboUsed.current = true;
        if (armed.current) disarm();
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (e.key !== 'Control') return;
      ctrlDown.current = false;

      if (isRecording) {
        const heldMs = Date.now() - recordingStartedAt.current;
        setUiState('hidden');
        armed.current = false;
        if (heldMs < MIN_RECORDING_MS) {
          onCancel();
        } else {
          onStop();
        }
        return;
      }

      if (comboUsed.current) {
        // Это был Ctrl+что-то, тапом не считаем.
        return;
      }

      const now = Date.now();
      if (now - lastCtrlUpAt.current <= DOUBLE_TAP_WINDOW_MS) {
        armed.current = true;
        setUiState('armed');
        clearArmedTimeout();
        armedTimeout.current = window.setTimeout(disarm, ARMED_TIMEOUT_MS);
      }
      lastCtrlUpAt.current = now;
    };

    const handleBlur = () => {
      // Окно потеряло фокус, например Alt+Tab с зажатым Ctrl, сбрасываем всё.
      ctrlDown.current = false;
      if (isRecording) {
        setUiState('hidden');
        armed.current = false;
        onCancel();
      } else {
        disarm();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);
    window.addEventListener('blur', handleBlur);

    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('keyup', handleKeyUp);
      window.removeEventListener('blur', handleBlur);
      clearArmedTimeout();
    };
  }, [isRecording, onStart, onStop, onCancel]);

  return { uiState };
}
