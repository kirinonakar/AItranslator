import type { Language, Provider } from "./types";

export const DEFAULT_BASE_URL = "http://localhost:1234/v1";
export const DEFAULT_LM_API_KEY = "lm-studio";
export const OLLAMA_BASE_URL = "http://localhost:11434/v1";
export const DEFAULT_OLLAMA_API_KEY = "ollama";

export const GOOGLE_BASE_URL = "https://generativelanguage.googleapis.com/v1beta/openai/";
export const GOOGLE_MODELS = [
  "gemini-flash-lite-latest",
  "gemini-flash-latest",
  "gemini-pro-latest",
  "gemma-4-26b-a4b-it",
  "gemma-4-31b-it",
];
export const DEFAULT_GOOGLE_MODEL = "gemini-flash-lite-latest";

export const LM_STUDIO_MODELS = [
  "google/gemma-4-e4b",
  "unsloth/gemma-4-26b-a4b-it",
  "translategemma-12b-it",
];

export const OLLAMA_MODELS = [
  "llama3.2",
  "gemma3",
  "qwen3",
  "gpt-oss:20b",
];
export const DEFAULT_OLLAMA_MODEL = "llama3.2";

export const OLLAMA_CLOUD_BASE_URL = "https://ollama.com/v1";
export const OLLAMA_CLOUD_MODELS = [
  "gemma4:31b",
  "gpt-oss:120b",
  "gpt-oss:20b",
  "gemma3:27b",
];
export const DEFAULT_OLLAMA_CLOUD_MODEL = "gemma4:31b";

export const CEREBRAS_BASE_URL = "https://api.cerebras.ai/v1";
export const CEREBRAS_MODELS = [
  "gemma-4-31b",
  "gpt-oss-120b",
  "zai-glm-4.7",
];
export const DEFAULT_CEREBRAS_MODEL = "gemma-4-31b";

export const PROVIDERS: Provider[] = ["LM Studio", "Ollama", "Ollama Cloud", "Google", "Cerebras"];

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
