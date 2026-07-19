export interface Profile {
  id: string;
  name: string;
  systemPrompt: string;
  userContext: string;
  isDefault: boolean;
  createdAt: number;
}