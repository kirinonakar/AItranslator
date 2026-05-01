import type { Language, Provider } from "./types";

export const DEFAULT_BASE_URL = "http://localhost:1234/v1";
export const DEFAULT_LM_API_KEY = "lm-studio";

export const GOOGLE_BASE_URL = "https://generativelanguage.googleapis.com/v1beta/openai/";
export const GOOGLE_MODELS = [
  "gemini-3.1-flash-lite-preview",
  "gemini-3-flash-preview",
  "gemini-3.1-pro-preview",
  "gemini-2.5-flash",
  "gemini-2.5-flash-lite",
  "gemma-4-26b-a4b-it",
  "gemma-4-31b-it",
];
export const DEFAULT_GOOGLE_MODEL = "gemini-3.1-flash-lite-preview";

export const LM_STUDIO_MODELS = [
  "google/gemma-4-e4b",
  "unsloth/gemma-4-26b-a4b-it",
  "translategemma-12b-it",
];

export const PROVIDERS: Provider[] = ["LM Studio", "Google"];

export const SOURCE_LANGUAGES: Language[] = [
  "Auto Detect",
  "English",
  "Korean",
  "Japanese",
  "Chinese",
  "Spanish",
  "French",
  "German",
  "Russian",
];

export const TARGET_LANGUAGES: Language[] = [
  "English",
  "Korean",
  "Japanese",
  "Chinese",
  "Spanish",
  "French",
  "German",
  "Russian",
];

export const SUPPORTED_FILE_TYPES = ".txt,.md,.py,.js,.html,.json,.csv";
