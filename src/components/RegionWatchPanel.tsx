import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type WatchRect = { x: number; y: number; width: number; height: number };

export type RegionWatchConfig = {
  rect: WatchRect | null;
  prompt: string;
  use_profile: boolean;
  interval_ms: number;
};

const MIN_INTERVAL_MS = 300;
const MIN_SELECTION_SIZE = 10;
const HANDLE_SIZE = 8;

type DragMode =
  | null
  | 'move'
  | 'nw'
  | 'n'
  | 'ne'
  | 'e'
  | 'se'
  | 's'
  | 'sw'
  | 'w';

type Props = {
  onClose: () => void;
};

export function RegionWatchPanel({ onClose }: Props) {
  const [config, setConfig] = useState<RegionWatchConfig | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [saving, setSaving] = useState(false);
  const [selecting, setSelecting] = useState(false);
  const [screenshotUrl, setScreenshotUrl] = useState<string | null>(null);
  const [selectionRect, setSelectionRect] = useState<WatchRect | null>(null);
  const [isDragging, setIsDragging] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);

  const dragStateRef = useRef<{
    mode: DragMode;
    startX: number;
    startY: number;
    startRect: WatchRect;
    containerRect: DOMRect;
  } | null>(null);

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

  const handleSelectArea = useCallback(async () => {
    setSelecting(true);
    try {
      await invoke('start_region_selection');
      // Poll until user finishes selection or timeout
      const poll = setInterval(async () => {
        try {
          const cfg = await invoke<RegionWatchConfig>('get_region_watch_config');
          setConfig(cfg);
          if (cfg.rect) {
            clearInterval(poll);
            setSelecting(false);
          }
        } catch {
          clearInterval(poll);
          setSelecting(false);
        }
      }, 500);
      setTimeout(() => {
        clearInterval(poll);
        setSelecting(false);
      }, 30000);
    } catch (e) {
      console.error('Failed to start region selection:', e);
      setSelecting(false);
    }
  }, []);

  const handleImageLoad = useCallback(() => {
    const img = imgRef.current;
    const container = containerRef.current;
    if (!img || !container) return;

    const imgRect = img.getBoundingClientRect();
    const containerRect = container.getBoundingClientRect();

    const offsetX = imgRect.left - containerRect.left;
    const offsetY = imgRect.top - containerRect.top;

    const w = Math.min(400, imgRect.width * 0.5);
    const h = Math.min(300, imgRect.height * 0.5);

    setSelectionRect({
      x: offsetX + (imgRect.width - w) / 2,
      y: offsetY + (imgRect.height - h) / 2,
      width: w,
      height: h,
    });
  }, []);

  const handleConfirmSelection = useCallback(() => {
    if (!selectionRect || !containerRef.current || !imgRef.current) return;

    const img = imgRef.current;
    const container = containerRef.current;

    const imgRect = img.getBoundingClientRect();
    const containerRect = container.getBoundingClientRect();

    if (img.naturalWidth === 0 || img.naturalHeight === 0) {
      console.error('Image not loaded yet');
      return;
    }

    const scaleX = img.naturalWidth / imgRect.width;
    const scaleY = img.naturalHeight / imgRect.height;

    if (!Number.isFinite(scaleX) || !Number.isFinite(scaleY)) {
      console.error('Invalid image scale');
      return;
    }

    const relX = (selectionRect.x - (imgRect.left - containerRect.left)) * scaleX;
    const relY = (selectionRect.y - (imgRect.top - containerRect.top)) * scaleY;
    const relW = selectionRect.width * scaleX;
    const relH = selectionRect.height * scaleY;

    invoke<[number, number, number, number]>('get_virtual_desktop_size')
      .then(([vdX, vdY, vdW, vdH]) => {
        const globalX = Math.round(vdX + relX);
        const globalY = Math.round(vdY + relY);
        const globalW = Math.round(relW);
        const globalH = Math.round(relH);

        if (!Number.isFinite(globalX) || !Number.isFinite(globalY) ||
            !Number.isFinite(globalW) || !Number.isFinite(globalH)) {
          throw new Error('Calculated rect contains NaN/Infinity');
        }

        const clampedX = Math.max(vdX, Math.min(globalX, vdX + vdW - 1));
        const clampedY = Math.max(vdY, Math.min(globalY, vdY + vdH - 1));
        const clampedW = Math.max(MIN_SELECTION_SIZE, Math.min(globalW, vdX + vdW - clampedX));
        const clampedH = Math.max(MIN_SELECTION_SIZE, Math.min(globalH, vdY + vdH - clampedY));

        const newRect: WatchRect = {
          x: clampedX,
          y: clampedY,
          width: clampedW,
          height: clampedH,
        };

        if (config) {
          persist({ ...config, rect: newRect });
        }
        setScreenshotUrl(null);
        setSelectionRect(null);
      })
      .catch((e) => {
        console.error('Failed to confirm selection:', e);
        setScreenshotUrl(null);
        setSelectionRect(null);
      });
  }, [selectionRect, config, persist]);

  const handleCancelSelection = useCallback(() => {
    setScreenshotUrl(null);
    setSelectionRect(null);
  }, []);

  const getDragMode = (clientX: number, clientY: number, rect: WatchRect): DragMode => {
    const handleOffset = HANDLE_SIZE / 2 + 2;
    const left = rect.x;
    const right = rect.x + rect.width;
    const top = rect.y;
    const bottom = rect.y + rect.height;

    const near = (a: number, b: number) => Math.abs(a - b) <= handleOffset;

    const onLeft = near(clientX, left);
    const onRight = near(clientX, right);
    const onTop = near(clientY, top);
    const onBottom = near(clientY, bottom);

    if (onTop && onLeft) return 'nw';
    if (onTop && onRight) return 'ne';
    if (onBottom && onLeft) return 'sw';
    if (onBottom && onRight) return 'se';
    if (onTop) return 'n';
    if (onBottom) return 's';
    if (onLeft) return 'w';
    if (onRight) return 'e';

    if (clientX >= left && clientX <= right && clientY >= top && clientY <= bottom) {
      return 'move';
    }
    return null;
  };

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (!selectionRect || !containerRef.current) return;
      const containerRect = containerRef.current.getBoundingClientRect();
      const localX = e.clientX - containerRect.left;
      const localY = e.clientY - containerRect.top;

      const mode = getDragMode(localX, localY, selectionRect);
      if (!mode) return;

      e.preventDefault();
      e.stopPropagation();

      dragStateRef.current = {
        mode,
        startX: localX,
        startY: localY,
        startRect: { ...selectionRect },
        containerRect,
      };
      setIsDragging(true);
    },
    [selectionRect],
  );

  const handleMouseMove = useCallback(
    (e: MouseEvent) => {
      if (!dragStateRef.current || !selectionRect) return;
      const { mode, startX, startY, startRect, containerRect } = dragStateRef.current;
      const localX = e.clientX - containerRect.left;
      const localY = e.clientY - containerRect.top;
      const dx = localX - startX;
      const dy = localY - startY;

      let nextRect = { ...startRect };

      switch (mode) {
        case 'move':
          nextRect.x = startRect.x + dx;
          nextRect.y = startRect.y + dy;
          break;
        case 'nw':
          nextRect.x = startRect.x + dx;
          nextRect.y = startRect.y + dy;
          nextRect.width = Math.max(MIN_SELECTION_SIZE, startRect.width - dx);
          nextRect.height = Math.max(MIN_SELECTION_SIZE, startRect.height - dy);
          break;
        case 'n':
          nextRect.y = startRect.y + dy;
          nextRect.height = Math.max(MIN_SELECTION_SIZE, startRect.height - dy);
          break;
        case 'ne':
          nextRect.y = startRect.y + dy;
          nextRect.width = Math.max(MIN_SELECTION_SIZE, startRect.width + dx);
          nextRect.height = Math.max(MIN_SELECTION_SIZE, startRect.height - dy);
          break;
        case 'e':
          nextRect.width = Math.max(MIN_SELECTION_SIZE, startRect.width + dx);
          break;
        case 'se':
          nextRect.width = Math.max(MIN_SELECTION_SIZE, startRect.width + dx);
          nextRect.height = Math.max(MIN_SELECTION_SIZE, startRect.height + dy);
          break;
        case 's':
          nextRect.height = Math.max(MIN_SELECTION_SIZE, startRect.height + dy);
          break;
        case 'sw':
          nextRect.x = startRect.x + dx;
          nextRect.width = Math.max(MIN_SELECTION_SIZE, startRect.width - dx);
          nextRect.height = Math.max(MIN_SELECTION_SIZE, startRect.height + dy);
          break;
        case 'w':
          nextRect.x = startRect.x + dx;
          nextRect.width = Math.max(MIN_SELECTION_SIZE, startRect.width - dx);
          break;
      }

      setSelectionRect(nextRect);
    },
    [selectionRect],
  );

  const handleMouseUp = useCallback(() => {
    dragStateRef.current = null;
    setIsDragging(false);
  }, []);

  useEffect(() => {
    if (!isDragging) return;
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isDragging, handleMouseMove, handleMouseUp]);

  const handleToggleEnabled = useCallback(async () => {
    const next = !enabled;
    setEnabled(next);
    await invoke('set_region_watch_enabled', { enabled: next });
  }, [enabled]);

  if (!config) return null;

  return (
    <div className="p-4 space-y-3 text-sm">
      <div className="flex items-center justify-between">
        <h3 className="text-surface-fg font-medium">Слежение за областью экрана</h3>
        <button
          type="button"
          onClick={onClose}
          className="text-xs text-surface-muted hover:text-surface-fg"
        >
          Закрыть
        </button>
      </div>

      {screenshotUrl ? (
        <div className="space-y-2">
          <div
            ref={containerRef}
            className="relative border border-surface-border rounded-md overflow-hidden cursor-crosshair select-none"
            onMouseDown={handleMouseDown}
          >
            <img
              ref={imgRef}
              src={screenshotUrl}
              alt="Desktop preview"
              className="block w-full max-h-[70vh] object-contain"
              draggable={false}
              onLoad={handleImageLoad}
            />
            {selectionRect && (
              <div
                className="absolute border-2 border-red-500 bg-red-500/10 pointer-events-none"
                style={{
                  left: selectionRect.x,
                  top: selectionRect.y,
                  width: selectionRect.width,
                  height: selectionRect.height,
                }}
              >
                {(['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'] as const).map((h) => {
                  const pos: Record<string, React.CSSProperties> = {
                    nw: { left: -HANDLE_SIZE / 2, top: -HANDLE_SIZE / 2 },
                    n: { left: '50%', top: -HANDLE_SIZE / 2, transform: 'translateX(-50%)' },
                    ne: { right: -HANDLE_SIZE / 2, top: -HANDLE_SIZE / 2 },
                    e: { right: -HANDLE_SIZE / 2, top: '50%', transform: 'translateY(-50%)' },
                    se: { right: -HANDLE_SIZE / 2, bottom: -HANDLE_SIZE / 2 },
                    s: { left: '50%', bottom: -HANDLE_SIZE / 2, transform: 'translateX(-50%)' },
                    sw: { left: -HANDLE_SIZE / 2, bottom: -HANDLE_SIZE / 2 },
                    w: { left: -HANDLE_SIZE / 2, top: '50%', transform: 'translateY(-50%)' },
                  };
                  return (
                    <div
                      key={h}
                      className="absolute bg-red-500 rounded-full"
                      style={{ width: HANDLE_SIZE, height: HANDLE_SIZE, ...pos[h] }}
                    />
                  );
                })}
              </div>
            )}
          </div>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={handleConfirmSelection}
              className="px-3 py-1.5 rounded-md bg-blue-500/20 text-blue-400 text-xs hover:bg-blue-500/30"
            >
              Подтвердить
            </button>
            <button
              type="button"
              onClick={handleCancelSelection}
              className="px-3 py-1.5 rounded-md border border-surface-border text-xs hover:bg-surface-hover"
            >
              Отмена
            </button>
          </div>
          <p className="text-xs text-surface-muted">
            Тащите прямоугольник за красную рамку (перемещение) или за уголки/стороны (изменение размера).
          </p>
        </div>
      ) : (
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleSelectArea}
            disabled={selecting}
            className="px-3 py-1.5 rounded-md border border-surface-border text-xs hover:bg-surface-hover disabled:opacity-40"
          >
            {selecting
              ? 'Захват экрана…'
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
      )}

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
          onChange={(e) => setConfig({ ...config, interval_ms: Number(e.target.value) })}
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