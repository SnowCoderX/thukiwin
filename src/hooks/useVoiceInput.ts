import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type VoiceInputStatus = 'idle' | 'recording' | 'finishing' | 'error';

interface VoiceChunkPayload {
  session_id: number;
  text: string;
  is_final: boolean;
}

const VOICE_CHUNK_EVENT = 'thuki://voice-chunk';
const VOICE_LEVEL_EVENT = 'thuki://voice-level';

/**
 * Голосовой ввод с расшифровкой в реальном времени.
 *
 * Архитектура:
 * - Внутреннее состояние операций (opStateRef) отслеживает реальный статус
 *   IPC-вызовов: 'idle' | 'pending' | 'recording' | 'stopping'.
 *   Это критично, потому что горячие клавиши и клики мыши могут приходить
 *   между рендерами, и closure со старым state даст race condition.
 * - React state (status, volume) используется ТОЛЬКО для UI-отображения.
 *   'error' — это UI-статус; операционно после ошибки мы сразу в 'idle'.
 */
export function useVoiceInput(onChunk: (text: string, sessionId: number, isFinal: boolean) => void) {
  const [status, setStatus] = useState<VoiceInputStatus>('idle');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [volume, setVolume] = useState(0);

  const onChunkRef = useRef(onChunk);
  onChunkRef.current = onChunk;

  // === Атомарное состояние для операций (не зависит от рендера) ===
  const opStateRef = useRef<'idle' | 'pending' | 'recording' | 'stopping'>('idle');

  useEffect(() => {
    const unlistenChunk = listen<VoiceChunkPayload>(VOICE_CHUNK_EVENT, (event) => {
      const { session_id, text, is_final } = event.payload;
      // is_final: false — промежуточный чанк (новые слова), дописываем
      // is_final: true — финальный чанк (полный текст), заменяем целиком
      if (text.trim() || is_final) {
        onChunkRef.current(text.trim(), session_id, is_final);
      }
      if (is_final) {
        opStateRef.current = 'idle';
        setStatus('idle');
      }
    });

    const unlistenLevel = listen<{ session_id: number; level: number }>(VOICE_LEVEL_EVENT, (event) => {
      const normalized = Math.min(100, Math.round((event.payload.level / 0.05) * 100));
      setVolume(normalized);
    });

    return () => {
      void unlistenChunk.then((u) => u());
      void unlistenLevel.then((u) => u());
    };
  }, []);

  const start = useCallback(async () => {
    // Атомарная проверка через ref — не через closure state
    if (opStateRef.current !== 'idle') {
      console.warn('[voice] start ignored, opState:', opStateRef.current);
      return;
    }
    console.log('[voice] starting...');
    opStateRef.current = 'pending';
    setErrorMessage(null);
    setStatus('recording');
    try {
      await invoke('start_voice_recording');
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
      console.log('[voice] cancelled');
    } catch (err) {
      console.error('[voice] cancel failed:', err);
      opStateRef.current = 'idle';
      setStatus('idle');
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
    start,
    stop,
    cancel,
    toggle,
  };
}