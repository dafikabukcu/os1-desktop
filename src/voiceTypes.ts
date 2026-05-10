export type AssistantIdentity = {
  assistantName: string;
  profileSlug: string;
  distro: string;
  profileReady?: boolean;
};

export type PersonalityMode = "assistant" | "her";

