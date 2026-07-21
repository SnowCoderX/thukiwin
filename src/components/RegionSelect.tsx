import { useEffect, useRef, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

export default function RegionSelect() {
  const [selection, setSelection] = useState<{ x: number; y: number; w: number; h: number } | null>(null);
  const startPos = useRef<{ x: number; y: number } | null>(null);
  const isDragging = useRef(false);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    isDragging.current = true;
    startPos.current = { x: e.clientX, y: e.clientY };
    setSelection({ x: e.clientX, y: e.clientY, w: 0, h: 0 });
  }, []);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (!isDragging.current || !startPos.current) return;
    const x = Math.min(startPos.current.x, e.clientX);
    const y = Math.min(startPos.current.y, e.clientY);
    const w = Math.abs(e.clientX - startPos.current.x);
    const h = Math.abs(e.clientY - startPos.current.y);
    setSelection({ x, y, w, h });
  }, []);

  const handleMouseUp = useCallback(() => {
    if (!isDragging.current) return;
    isDragging.current = false;
    const sel = selection;
    setSelection(null);

    if (!sel || sel.w < 10 || sel.h < 10) {
      invoke('cancel_region_selection').catch(console.error);
      return;
    }

    invoke('finish_region_selection', {
      x: Math.round(sel.x),
      y: Math.round(sel.y),
      width: Math.round(sel.w),
      height: Math.round(sel.h),
    }).catch(console.error);
  }, [selection]);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      invoke('cancel_region_selection').catch(console.error);
    }
  }, []);

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  return (
    <div
      className="fixed inset-0 cursor-crosshair select-none"
      style={{
        background: 'rgba(0, 0, 0, 0.35)',
        outline: 'none',
        border: 'none',
        boxShadow: 'none'
      }}
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
    >
      {selection && selection.w > 0 && selection.h > 0 && (
        <div
          className="absolute pointer-events-none"
          style={{
            left: selection.x,
            top: selection.y,
            width: selection.w,
            height: selection.h,
            border: '2px solid #3b82f6',
            backgroundColor: 'rgba(59, 130, 246, 0.2)',
            boxShadow: '0 0 0 9999px rgba(0, 0, 0, 0.35)',
            outline: 'none',
          }}
        />
      )}
      <div
        className="absolute bottom-10 left-1/2 -translate-x-1/2 px-5 py-2 rounded-xl text-white text-sm font-medium select-none pointer-events-none"
        style={{ background: 'rgba(0, 0, 0, 0.8)', outline: 'none' }}
      >
        🖱 Выделите область • Esc для отмены
      </div>
    </div>
  );
}