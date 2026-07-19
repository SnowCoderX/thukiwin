import { useState, useCallback, useEffect } from 'react';
import type { Profile } from '../types/profile';
import { PROFILES_STORAGE_KEY, ACTIVE_PROFILE_STORAGE_KEY } from '../config';

const DEFAULT_PROFILE: Profile = {
  id: 'default',
  name: 'General',
  systemPrompt: '',
  userContext: '',
  isDefault: true,
  createdAt: Date.now(),
};

function loadProfiles(): Profile[] {
  try {
    const raw = localStorage.getItem(PROFILES_STORAGE_KEY);
    if (!raw) return [DEFAULT_PROFILE];
    const parsed = JSON.parse(raw) as Profile[];
    return parsed.length > 0 ? parsed : [DEFAULT_PROFILE];
  } catch {
    return [DEFAULT_PROFILE];
  }
}

function loadActiveProfileId(): string | null {
  try {
    return localStorage.getItem(ACTIVE_PROFILE_STORAGE_KEY);
  } catch {
    return null;
  }
}

export function useProfiles() {
  const [profiles, setProfiles] = useState<Profile[]>(loadProfiles);
  const [activeProfileId, setActiveProfileId] = useState<string | null>(loadActiveProfileId);

  useEffect(() => {
    localStorage.setItem(PROFILES_STORAGE_KEY, JSON.stringify(profiles));
  }, [profiles]);

  useEffect(() => {
    if (activeProfileId) {
      localStorage.setItem(ACTIVE_PROFILE_STORAGE_KEY, activeProfileId);
    } else {
      localStorage.removeItem(ACTIVE_PROFILE_STORAGE_KEY);
    }
  }, [activeProfileId]);

  const activeProfile =
    profiles.find((p) => p.id === activeProfileId) ??
    profiles.find((p) => p.isDefault) ??
    profiles[0] ??
    DEFAULT_PROFILE;

  const createProfile = useCallback(
    (data: Omit<Profile, 'id' | 'createdAt'>): Profile => {
      const newProfile: Profile = {
        ...data,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
      };
      setProfiles((prev) => [...prev, newProfile]);
      return newProfile;
    },
    [],
  );

  const updateProfile = useCallback(
    (id: string, updates: Partial<Omit<Profile, 'id' | 'createdAt'>>) => {
      setProfiles((prev) =>
        prev.map((p) => (p.id === id ? { ...p, ...updates } : p)),
      );
    },
    [],
  );

  const deleteProfile = useCallback((id: string) => {
    let nextProfiles: Profile[] = [];
    setProfiles((prev) => {
      const deletedWasDefault = prev.find((p) => p.id === id)?.isDefault ?? false;
      const filtered = prev.filter((p) => p.id !== id);
      if (filtered.length === 0) {
        nextProfiles = [DEFAULT_PROFILE];
        return nextProfiles;
      }
      if (deletedWasDefault && !filtered.some((p) => p.isDefault)) {
        filtered[0] = { ...filtered[0], isDefault: true };
      }
      nextProfiles = filtered;
      return filtered;
    });
    setActiveProfileId((current) => {
      if (current === id) {
        const fallback =
          nextProfiles.find((p) => p.isDefault) ?? nextProfiles[0] ?? DEFAULT_PROFILE;
        return fallback.id;
      }
      return current;
    });
}, []);

  const duplicateProfile = useCallback(
    (id: string) => {
      const source = profiles.find((p) => p.id === id);
      if (!source) return;
      const copy: Profile = {
        ...source,
        id: crypto.randomUUID(),
        name: `${source.name} (Copy)`,
        isDefault: false,
        createdAt: Date.now(),
      };
      setProfiles((prev) => [...prev, copy]);
    },
    [profiles],
  );

  const setDefaultProfile = useCallback((id: string) => {
    setProfiles((prev) =>
      prev.map((p) => ({ ...p, isDefault: p.id === id })),
    );
  }, []);

  const setActiveProfile = useCallback((id: string | null) => {
    setActiveProfileId(id);
  }, []);

  const getProfileSystemPrompt = useCallback((): string | undefined => {
    if (!activeProfile || activeProfile.id === 'default') return undefined;
    const parts: string[] = [];
    if (activeProfile.systemPrompt.trim()) {
      parts.push(
        `# Profile: ${activeProfile.name}\n${activeProfile.systemPrompt}`,
      );
    }
    if (activeProfile.userContext.trim()) {
      parts.push(`# User Context\n${activeProfile.userContext}`);
    }
    return parts.length > 0 ? parts.join('\n\n') : undefined;
  }, [activeProfile]);

  return {
    profiles,
    activeProfile,
    activeProfileId,
    setActiveProfile,
    createProfile,
    updateProfile,
    deleteProfile,
    duplicateProfile,
    setDefaultProfile,
    getProfileSystemPrompt,
  };
}