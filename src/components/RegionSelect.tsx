import { useEffect, useRef, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

/**
 * Full-screen overlay for region selection.
 * Rendered inside the "region-select" Tauri window (see start_region_selection
 * in lib.rs). Tracks mouse drag to build a rectangle and sends it back to
 * Rust via finish_region_selection. Escape cancels.
 */
export default function RegionSelect() {
  const containerRef = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState<{
    x: number;
    y: number;
    w: number;
    h: number;
  } | null>(null);
  const startPos = useRef<{ x: number; y: number } | null>(null);
  const isDragging = useRef(false);

  useEffect(() => {
    // Grab focus so keyboard events work immediately
    getCurrentWindow().setFocus().catch(() => {});
  }, []);

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
      // Too small — treat as cancel
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
      ref={containerRef}
      className="fixed inset-0 cursor-crosshair select-none"
      style={{ background: 'rgba(0,0,0,0.01)' }}
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
            background: 'rgba(59,130,246,0.15)',
            border: '2px dashed rgba(255,255,255,0.9)',
            boxShadow: '0 0 0 9999px rgba(0,0,0,0.35)',
          }}
        />
      )}
      <div
        className="absolute bottom-6 left-1/2 -translate-x-1/2 px-3 py-1.5 rounded-full text-white/80 text-xs font-medium select-none pointer-events-none"
        style={{ background: 'rgba(0,0,0,0.6)', backdropFilter: 'blur(4px)' }}
      >
        Drag to select region • Esc to cancel
      </div>
    </div>
  );
}