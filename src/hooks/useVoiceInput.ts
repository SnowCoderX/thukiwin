import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type VoiceInputStatus = 'idle' | 'recording' | 'finishing' | 'error';

interface VoiceChunkPayload {
  text: string;
  is_final: boolean;
}

const VOICE_CHUNK_EVENT = 'thuki://voice-chunk';

/**
 * Голосовой ввод с расшифровкой в реальном времени.
 *
 * Пока идёт запись, бэкенд (whisper-rs, см. `src-tauri/src/voice.rs`) сам
 * режет звук по паузам (~2.5с тишины) и присылает расшифрованные куски по
 * мере готовности через событие `thuki://voice-chunk`. Каждый кусок только
 * добавляется в поле ввода через onChunk, отправка сообщения сюда не
 * входит, это решает пользователь вручную.
 *
 * `start`/`stop` для push-to-talk сценариев (зажал, отпустил), `toggle` для
 * обычной кнопки микрофона (тап, тап). `cancel` отменяет запись целиком,
 * без расшифровки хвоста, например если запись оказалась слишком короткой.
 */
export function useVoiceInput(onChunk: (text: string) => void) {
  const [status, setStatus] = useState<VoiceInputStatus>('idle');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const onChunkRef = useRef(onChunk);
  onChunkRef.current = onChunk;

  useEffect(() => {
    const unlistenPromise = listen<VoiceChunkPayload>(VOICE_CHUNK_EVENT, (event) => {
      const { text, is_final } = event.payload;
      if (text.trim()) {
        onChunkRef.current(text.trim());
      }
      if (is_final) {
        setStatus('idle');
      }
    });

    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const start = useCallback(async () => {
    setErrorMessage(null);
    try {
      await invoke('start_voice_recording');
      setStatus('recording');
    } catch (err) {
      setStatus('error');
      setErrorMessage(String(err));
    }
  }, []);

  const stop = useCallback(async () => {
    setStatus('finishing');
    try {
      await invoke('stop_voice_recording');
      // статус вернётся в 'idle' по событию с is_final: true
    } catch (err) {
      setStatus('error');
      setErrorMessage(String(err));
    }
  }, []);

  const cancel = useCallback(async () => {
    try {
      await invoke('cancel_voice_recording');
    } finally {
      setStatus('idle');
    }
  }, []);

  /** Стартует или останавливает запись в зависимости от текущего статуса. */
  const toggle = useCallback(() => {
    if (status === 'recording') {
      void stop();
    } else if (status === 'idle' || status === 'error') {
      void start();
    }
    // Пока 'finishing', тапы игнорируются до ответа бэкенда.
  }, [status, start, stop]);

  return { status, errorMessage, toggle, start, stop, cancel };
}