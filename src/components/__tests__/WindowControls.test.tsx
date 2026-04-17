import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { WindowControls } from '../WindowControls';

describe('WindowControls', () => {
  it('close button calls onClose when clicked', () => {
    const onClose = vi.fn();
    render(<WindowControls onClose={onClose} onMinimize={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Close window' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('close button has Windows close styling', () => {
    render(<WindowControls onClose={vi.fn()} onMinimize={vi.fn()} />);
    const closeBtn = screen.getByRole('button', { name: 'Close window' });
    expect(closeBtn).toHaveClass('win-title-btn-close');
  });

  it('renders minimize button', () => {
    render(<WindowControls onClose={vi.fn()} onMinimize={vi.fn()} />);
    expect(
      screen.getByRole('button', { name: 'Minimize' }),
    ).toBeInTheDocument();
  });

  it('minimize button calls onMinimize when clicked', () => {
    const onMinimize = vi.fn();
    render(<WindowControls onClose={vi.fn()} onMinimize={onMinimize} />);
    fireEvent.click(screen.getByRole('button', { name: 'Minimize' }));
    expect(onMinimize).toHaveBeenCalledTimes(1);
  });

  it('renders divider separator (bg-surface-border)', () => {
    const { container } = render(
      <WindowControls onClose={vi.fn()} onMinimize={vi.fn()} />,
    );
    expect(container.querySelector('.bg-surface-border')).not.toBeNull();
  });

  it('close button has x icon svg', () => {
    render(<WindowControls onClose={vi.fn()} onMinimize={vi.fn()} />);
    const closeBtn = screen.getByRole('button', { name: 'Close window' });
    const svg = closeBtn.querySelector('svg');
    expect(svg).not.toBeNull();
  });

  it('save button shows "Save conversation" aria-label when not saved', () => {
    render(
      <WindowControls
        onClose={vi.fn()}
        onMinimize={vi.fn()}
        onSave={vi.fn()}
        canSave
        isSaved={false}
      />,
    );
    expect(
      screen.getByRole('button', { name: 'Save conversation' }),
    ).toBeInTheDocument();
  });

  it('save button shows "Remove from history" aria-label when saved', () => {
    render(
      <WindowControls
        onClose={vi.fn()}
        onMinimize={vi.fn()}
        onSave={vi.fn()}
        canSave
        isSaved
      />,
    );
    expect(
      screen.getByRole('button', { name: 'Remove from history' }),
    ).toBeInTheDocument();
  });

  it('save button calls onSave when clicked while saved', () => {
    const onSave = vi.fn();
    render(
      <WindowControls
        onClose={vi.fn()}
        onMinimize={vi.fn()}
        onSave={onSave}
        canSave
        isSaved
      />,
    );
    fireEvent.click(
      screen.getByRole('button', { name: 'Remove from history' }),
    );
    expect(onSave).toHaveBeenCalledTimes(1);
  });
});
