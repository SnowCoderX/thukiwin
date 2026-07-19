import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

type WatchRect = { x: number; y: number; width: number; height: number };

export type RegionWatchConfig = {
  rect: WatchRect | null;
  prompt: string;
  use_profile: boolean;
  interval_ms: number;
};

const MIN_INTERVAL_MS = 300;

type Props = {
  onClose: () => void;
};

/**
 * Инлайн-панель настроек "слежения за областью экрана" — по аналогии с
 * `ProfileManagerPanel`, открывается между чатом и AskBarView.
 *
 * Загружает текущий конфиг при монтировании (панель открывается и
 * закрывается, а не живёт постоянно, так что useEffect-на-маунт достаточно —
 * не нужно синхронизироваться с бэкендом в реальном времени, кроме как
 * после `start_region_selection`, см. `refreshAfterSelection`).
 */
export function RegionWatchPanel({ onClose }: Props) {
  const [config, setConfig] = useState<RegionWatchConfig | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [saving, setSaving] = useState(false);
  const [selecting, setSelecting] = useState(false);

  const refresh = useCallback(async () => {
    const [cfg, on] = await Promise.all([
      invoke<RegionWatchConfig>('get_region_watch_config'),
      invoke<boolean>('get_region_watch_enabled'),
    ]);
    setConfig(cfg);
    setEnabled(on);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const persist = useCallback(async (next: RegionWatchConfig) => {
    setConfig(next);
    setSaving(true);
    try {
      await invoke('set_region_watch_config', { config: next });
    } finally {
      setSaving(false);
    }
  }, []);

  /**
   * Открывает окно выделения области. Rust закрывает то окно сам после
   * `finish_region_selection`/`cancel_region_selection` — здесь просто
   * слушаем его закрытие (`tauri://destroyed`) и перечитываем конфиг,
   * чтобы подхватить новый rect, если он был сохранён.
   */
  const handleSelectArea = useCallback(async () => {
    setSelecting(true);
    await invoke('start_region_selection');
    const win = await WebviewWindow.getByLabel('region-select');
    if (!win) {
      // Уже закрылось (или не открылось) быстрее, чем мы успели подписаться.
      setSelecting(false);
      void refresh();
      return;
    }
    await win.once('tauri://destroyed', () => {
      setSelecting(false);
      void refresh();
    });
  }, [refresh]);

  const handleToggleEnabled = useCallback(async () => {
    const next = !enabled;
    setEnabled(next);
    await invoke('set_region_watch_enabled', { enabled: next });
  }, [enabled]);

  if (!config) return null;

  return (
    <div className="p-4 space-y-3 text-sm">
      <div className="flex items-center justify-between">
        <h3 className="text-surface-fg font-medium">
          Слежение за областью экрана
        </h3>
        <button
          type="button"
          onClick={onClose}
          className="text-xs text-surface-muted hover:text-surface-fg"
        >
          Закрыть
        </button>
      </div>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={handleSelectArea}
          disabled={selecting}
          className="px-3 py-1.5 rounded-md border border-surface-border text-xs hover:bg-surface-hover disabled:opacity-40"
        >
          {selecting
            ? 'Выделите область на экране…'
            : config.rect
              ? 'Выбрать область заново'
              : 'Выбрать область'}
        </button>
        <span className="text-xs text-surface-muted">
          {config.rect
            ? `${config.rect.width}×${config.rect.height} @ (${config.rect.x}, ${config.rect.y})`
            : 'Область не выбрана'}
        </span>
      </div>

      <textarea
        value={config.prompt}
        onChange={(e) => setConfig({ ...config, prompt: e.target.value })}
        onBlur={() => config && persist(config)}
        placeholder="Что делать с этой областью (например: «переведи субтитры»)"
        className="w-full rounded-md border border-surface-border bg-surface-base px-2 py-1.5 text-xs resize-none"
        rows={2}
      />

      <label className="flex items-center gap-2 text-xs">
        <input
          type="checkbox"
          checked={config.use_profile}
          onChange={(e) => persist({ ...config, use_profile: e.target.checked })}
        />
        Учитывать системный промпт активного профиля
      </label>

      <label className="flex items-center gap-2 text-xs">
        Интервал проверки (мс)
        <input
          type="number"
          min={MIN_INTERVAL_MS}
          step={100}
          value={config.interval_ms}
          onChange={(e) =>
            setConfig({ ...config, interval_ms: Number(e.target.value) })
          }
          onBlur={() => config && persist(config)}
          className="w-20 rounded-md border border-surface-border bg-surface-base px-2 py-1 text-xs"
        />
      </label>

      <button
        type="button"
        onClick={handleToggleEnabled}
        disabled={!config.rect || saving}
        className={`w-full py-1.5 rounded-md text-xs font-medium transition-colors ${
          enabled
            ? 'bg-red-500/20 text-red-400 hover:bg-red-500/30'
            : 'bg-blue-500/20 text-blue-400 hover:bg-blue-500/30'
        } disabled:opacity-40`}
      >
        {enabled ? 'Остановить слежение' : 'Включить слежение'}
      </button>
    </div>
  );
}
