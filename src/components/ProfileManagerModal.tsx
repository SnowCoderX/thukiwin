import { useState, useCallback } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import type { Profile } from '../types/profile';

interface ProfileManagerModalProps {
  isOpen: boolean;
  onClose: () => void;
  profiles: Profile[];
  activeProfileId: string | null;
  onCreate: (data: Omit<Profile, 'id' | 'createdAt'>) => void;
  onUpdate: (id: string, updates: Partial<Omit<Profile, 'id' | 'createdAt'>>) => void;
  onDelete: (id: string) => void;
  onDuplicate: (id: string) => void;
  onSetActive: (id: string | null) => void;
}

export function ProfileManagerModal({
  isOpen,
  onClose,
  profiles,
  activeProfileId,
  onCreate,
  onUpdate,
  onDelete,
  onDuplicate,
  onSetActive,
}: ProfileManagerModalProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);

  const [formData, setFormData] = useState({
    name: '',
    systemPrompt: '',
    userContext: '',
  });

  const startCreate = useCallback(() => {
    setFormData({ name: '', systemPrompt: '', userContext: '' });
    setIsCreating(true);
    setEditingId(null);
  }, []);

  const startEdit = useCallback((profile: Profile) => {
    setFormData({
      name: profile.name,
      systemPrompt: profile.systemPrompt,
      userContext: profile.userContext,
    });
    setEditingId(profile.id);
    setIsCreating(false);
  }, []);

  const handleSubmit = useCallback(() => {
    if (!formData.name.trim()) return;
    if (isCreating) {
      onCreate({
        name: formData.name.trim(),
        systemPrompt: formData.systemPrompt.trim(),
        userContext: formData.userContext.trim(),
        isDefault: profiles.length === 0,
      });
    } else if (editingId) {
      onUpdate(editingId, {
        name: formData.name.trim(),
        systemPrompt: formData.systemPrompt.trim(),
        userContext: formData.userContext.trim(),
      });
    }
    setIsCreating(false);
    setEditingId(null);
  }, [formData, isCreating, editingId, onCreate, onUpdate, profiles.length]);

  const handleCancel = useCallback(() => {
    setIsCreating(false);
    setEditingId(null);
  }, []);

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <motion.div
        initial={{ opacity: 0, scale: 0.95 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={{ opacity: 0, scale: 0.95 }}
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-lg max-h-[80vh] overflow-hidden rounded-xl bg-surface-base border border-surface-border shadow-chat flex flex-col"
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-surface-border">
          <h2 className="text-sm font-semibold text-text-primary">
            Profile Manager
          </h2>
          <button
            onClick={onClose}
            className="text-text-secondary hover:text-text-primary text-xs px-2 py-1"
          >
            ✕
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          {profiles.map((profile) => (
            <div
              key={profile.id}
              className={`rounded-lg border p-3 transition-colors ${
                activeProfileId === profile.id
                  ? 'border-primary/50 bg-primary/5'
                  : 'border-surface-border bg-surface-elevated'
              }`}
            >
              <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-2 min-w-0">
                  <span className="text-sm font-medium text-text-primary truncate">
                    {profile.name}
                  </span>
                  {profile.isDefault && (
                    <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-primary/20 text-primary shrink-0">
                      Default
                    </span>
                  )}
                  {activeProfileId === profile.id && (
                    <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-green-500/20 text-green-400 shrink-0">
                      Active
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  {activeProfileId !== profile.id && (
                    <button
                      onClick={() => onSetActive(profile.id)}
                      className="px-2 py-1 text-[10px] rounded bg-surface-base hover:bg-white/8 text-text-secondary transition-colors"
                    >
                      Activate
                    </button>
                  )}
                  <button
                    onClick={() => startEdit(profile)}
                    className="px-2 py-1 text-[10px] rounded bg-surface-base hover:bg-white/8 text-text-secondary transition-colors"
                  >
                    Edit
                  </button>
                  <button
                    onClick={() => onDuplicate(profile.id)}
                    className="px-2 py-1 text-[10px] rounded bg-surface-base hover:bg-white/8 text-text-secondary transition-colors"
                  >
                    Copy
                  </button>
                  {profiles.length > 1 && (
                    <button
                      onClick={() => onDelete(profile.id)}
                      className="px-2 py-1 text-[10px] rounded bg-red-500/10 hover:bg-red-500/20 text-red-400 transition-colors"
                    >
                      Del
                    </button>
                  )}
                </div>
              </div>
              {profile.systemPrompt && (
                <p className="text-[11px] text-text-secondary line-clamp-2 mb-1">
                  {profile.systemPrompt}
                </p>
              )}
              {profile.userContext && (
                <p className="text-[11px] text-text-tertiary line-clamp-1">
                  {profile.userContext}
                </p>
              )}
            </div>
          ))}

          <AnimatePresence>
            {(isCreating || editingId) && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: 'auto', opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                className="overflow-hidden"
              >
                <div className="rounded-lg border border-surface-border bg-surface-elevated p-3 space-y-3">
                  <input
                    type="text"
                    placeholder="Profile name (e.g., Python Tutor)"
                    value={formData.name}
                    onChange={(e) =>
                      setFormData((prev) => ({ ...prev, name: e.target.value }))
                    }
                    className="w-full bg-surface-base border border-surface-border rounded px-3 py-2 text-sm text-text-primary placeholder:text-text-tertiary outline-none focus:border-primary/50"
                  />
                  <textarea
                    placeholder="System prompt — what the AI should do (e.g., 'You are a patient Python tutor who explains concepts with examples...')"
                    value={formData.systemPrompt}
                    onChange={(e) =>
                      setFormData((prev) => ({
                        ...prev,
                        systemPrompt: e.target.value,
                      }))
                    }
                    rows={4}
                    className="w-full bg-surface-base border border-surface-border rounded px-3 py-2 text-sm text-text-primary placeholder:text-text-tertiary outline-none focus:border-primary/50 resize-none"
                  />
                  <textarea
                    placeholder="User context — who you are, your goals, preferences (e.g., 'Beginner, preparing for interviews, prefers short answers')"
                    value={formData.userContext}
                    onChange={(e) =>
                      setFormData((prev) => ({
                        ...prev,
                        userContext: e.target.value,
                      }))
                    }
                    rows={3}
                    className="w-full bg-surface-base border border-surface-border rounded px-3 py-2 text-sm text-text-primary placeholder:text-text-tertiary outline-none focus:border-primary/50 resize-none"
                  />
                  <div className="flex items-center justify-end gap-2">
                    <button
                      onClick={handleCancel}
                      className="px-3 py-1.5 text-xs rounded-lg bg-surface-base text-text-secondary hover:bg-white/8 transition-colors"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={handleSubmit}
                      disabled={!formData.name.trim()}
                      className="px-3 py-1.5 text-xs rounded-lg bg-primary text-neutral hover:bg-primary/90 transition-colors disabled:opacity-40"
                    >
                      {isCreating ? 'Create' : 'Save'}
                    </button>
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>

          {!isCreating && !editingId && (
            <button
              onClick={startCreate}
              className="w-full py-2 rounded-lg border border-dashed border-surface-border text-text-secondary hover:text-text-primary hover:border-text-secondary/30 transition-colors text-xs"
            >
              + New Profile
            </button>
          )}
        </div>
      </motion.div>
    </div>
  );
}