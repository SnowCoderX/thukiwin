import type React from 'react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

type Point = { x: number; y: number };

/**
 * Содержимое окна `region-select` (см. `start_region_selection` в lib.rs).
 *
 * Само окно уже растянуто на bounding box всех мониторов и позиционировано
 * в его левом верхнем углу, так что здесь достаточно работать в локальных
 * координатах вьюпорта — Rust сам прибавляет смещение при получении rect.
 *
 * mousedown → mousemove → mouseup рисует рамку и на отпускании отправляет
 * выделение в `finish_region_selection`. Esc отменяет через
 * `cancel_region_selection` без изменения текущей сохранённой области.
 * Оба команды сами закрывают это окно — здесь не нужно вызывать
 * `getCurrentWindow().close()` вручную.
 */
export function RegionSelectOverlay() {
  const [start, setStart] = useState<Point | null>(null);
  const [current, setCurrent] = useState<Point | null>(null);
  const draggingRef = useRef(false);

  const cancel = useCallback(() => {
    void invoke('cancel_region_selection');
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') cancel();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [cancel]);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    // Только левая кнопка — иначе правый клик для контекстного меню тоже
    // начинал бы рисовать рамку.
    if (e.button !== 0) return;
    draggingRef.current = true;
    const point = { x: e.clientX, y: e.clientY };
    setStart(point);
    setCurrent(point);
  }, []);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (!draggingRef.current) return;
    setCurrent({ x: e.clientX, y: e.clientY });
  }, []);

  const handleMouseUp = useCallback(() => {
    if (!draggingRef.current || !start || !current) return;
    draggingRef.current = false;

    const x = Math.round(Math.min(start.x, current.x));
    const y = Math.round(Math.min(start.y, current.y));
    const width = Math.round(Math.abs(current.x - start.x));
    const height = Math.round(Math.abs(current.y - start.y));

    void invoke('finish_region_selection', { x, y, width, height });
  }, [start, current]);

  const rect =
    start && current
      ? {
          left: Math.min(start.x, current.x),
          top: Math.min(start.y, current.y),
          width: Math.abs(current.x - start.x),
          height: Math.abs(current.y - start.y),
        }
      : null;

  return (
    <div
      className="fixed inset-0 overflow-hidden select-none"
      style={{ background: 'rgba(0, 0, 0, 0.15)', cursor: 'crosshair' }}
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
    >
      <div className="absolute top-6 left-1/2 -translate-x-1/2 px-4 py-2 rounded-lg bg-black/70 text-white text-sm pointer-events-none">
        Выделите область мышкой — Esc для отмены
      </div>
      {rect ? (
        <div
          className="absolute border-2 border-blue-400 bg-blue-400/10 pointer-events-none"
          style={{
            left: rect.left,
            top: rect.top,
            width: rect.width,
            height: rect.height,
          }}
        />
      ) : null}
    </div>
  );
}
