import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type VoiceInputStatus = 'idle' | 'recording' | 'finishing' | 'error';

interface VoiceChunkPayload {
  session_id: number;
  text: string;
  is_final: boolean;
}

interface VoiceCountdownPayload {
  session_id: number;
  fraction: number;
}

const VOICE_CHUNK_EVENT = 'thuki://voice-chunk';
const VOICE_LEVEL_EVENT = 'thuki://voice-level';
const VOICE_COUNTDOWN_EVENT = 'thuki://voice-countdown';

/** Options for starting a recording session. */
export interface StartVoiceOptions {
  /**
   * When true, the whole phrase is captured, transcribed once after a
   * silence pause, and delivered via `onChunk`'s `isFinal` callback with the
   * FULL text (instead of the empty-text final used by manual/Ctrl-hold
   * sessions, where text streams in via intermediate chunks instead).
   * Used for wake-word-triggered ("туки") hands-free recording.
   */
  autoSubmit?: boolean;
  /**
   * Text already captured together with the wake-word in the same breath
   * (e.g. "туки переведи слово apple") — prepended to whatever is heard
   * after the following silence pause. Only relevant when `autoSubmit`.
   */
  prefixText?: string;
}

/**
 * Голосовой ввод с расшифровкой в реальном времени.
 *
 * Архитектура:
 * - Внутреннее состояние операций (opStateRef) отслеживает реальный статус
 *   IPC-вызовов: 'idle' | 'pending' | 'recording' | 'stopping'.
 *   Это критично, потому что горячие клавиши, клики мыши и wake-word могут
 *   приходить между рендерами, и closure со старым state даст race condition.
 * - React state (status, volume, autoSendFraction) используется ТОЛЬКО для
 *   UI-отображения. 'error' — это UI-статус; операционно после ошибки мы
 *   сразу в 'idle'.
 */
export function useVoiceInput(onChunk: (text: string, sessionId: number, isFinal: boolean) => void) {
  const [status, setStatus] = useState<VoiceInputStatus>('idle');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [volume, setVolume] = useState(0);
  /** 0..1 — доля тишины до авто-отправки, только для авто-сессий (wake-word). */
  const [autoSendFraction, setAutoSendFraction] = useState(0);

  const onChunkRef = useRef(onChunk);
  onChunkRef.current = onChunk;

  // === Атомарное состояние для операций (не зависит от рендера) ===
  const opStateRef = useRef<'idle' | 'pending' | 'recording' | 'stopping'>('idle');

  useEffect(() => {
    const unlistenChunk = listen<VoiceChunkPayload>(VOICE_CHUNK_EVENT, (event) => {
      const { session_id, text, is_final } = event.payload;
      // is_final: false — промежуточный чанк (новые слова), дописываем
      // is_final: true — финальный чанк:
      //   - ручной режим: текст пустой (весь текст уже собран из чанков)
      //   - авто-режим (wake-word): текст — ПОЛНАЯ распознанная фраза
      if (text.trim() || is_final) {
        onChunkRef.current(text.trim(), session_id, is_final);
      }
      if (is_final) {
        opStateRef.current = 'idle';
        setStatus('idle');
        setAutoSendFraction(0);
      }
    });

    const unlistenLevel = listen<{ session_id: number; level: number }>(VOICE_LEVEL_EVENT, (event) => {
      const normalized = Math.min(100, Math.round((event.payload.level / 0.05) * 100));
      setVolume(normalized);
    });

    const unlistenCountdown = listen<VoiceCountdownPayload>(VOICE_COUNTDOWN_EVENT, (event) => {
      setAutoSendFraction(Math.max(0, Math.min(1, event.payload.fraction)));
    });

    return () => {
      void unlistenChunk.then((u) => u());
      void unlistenLevel.then((u) => u());
      void unlistenCountdown.then((u) => u());
    };
  }, []);

  const start = useCallback(async (opts?: StartVoiceOptions) => {
    // Атомарная проверка через ref — не через closure state
    if (opStateRef.current !== 'idle') {
      console.warn('[voice] start ignored, opState:', opStateRef.current);
      return;
    }
    console.log('[voice] starting...', opts);
    opStateRef.current = 'pending';
    setErrorMessage(null);
    setStatus('recording');
    setAutoSendFraction(0);
    try {
      await invoke('start_voice_recording', {
        autoSubmit: opts?.autoSubmit ?? false,
        prefixText: opts?.prefixText ?? '',
      });
      opStateRef.current = 'recording';
      console.log('[voice] recording started');
    } catch (err) {
      console.error('[voice] start failed:', err);
      opStateRef.current = 'idle';
      setStatus('error');
      setErrorMessage(String(err));
    }
  }, []);

  const stop = useCallback(async () => {
    if (opStateRef.current !== 'recording') {
      console.warn('[voice] stop ignored, opState:', opStateRef.current);
      return;
    }
    console.log('[voice] stopping...');
    opStateRef.current = 'stopping';
    setStatus('finishing');
    try {
      await invoke('stop_voice_recording');
      // Статус сбросится через is_final событие из Rust
      console.log('[voice] stop command sent');
    } catch (err) {
      console.error('[voice] stop failed:', err);
      opStateRef.current = 'idle';
      setStatus('error');
      setErrorMessage(String(err));
    }
  }, []);

  const cancel = useCallback(async () => {
    const current = opStateRef.current;
    if (current !== 'recording' && current !== 'pending' && current !== 'stopping') {
      console.warn('[voice] cancel ignored, opState:', current);
      return;
    }
    console.log('[voice] cancelling...');
    try {
      await invoke('cancel_voice_recording');
      opStateRef.current = 'idle';
      setStatus('idle');
      setAutoSendFraction(0);
      console.log('[voice] cancelled');
    } catch (err) {
      console.error('[voice] cancel failed:', err);
      opStateRef.current = 'idle';
      setStatus('idle');
      setAutoSendFraction(0);
    }
  }, []);

  const toggle = useCallback(async () => {
    const current = opStateRef.current;
    console.log('[voice] toggle, opState:', current);
    if (current === 'idle') {
      await start();
    } else if (current === 'recording') {
      await stop();
    }
    // 'pending' | 'stopping' — игнорируем, ждём завершения операции
  }, [start, stop]);

  return {
    status,
    errorMessage,
    volume,
    autoSendFraction,
    start,
    stop,
    cancel,
    toggle,
  };
}
