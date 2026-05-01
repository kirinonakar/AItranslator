export type Provider = "LM Studio" | "Google";
export type Language =
  | "Auto Detect"
  | "English"
  | "Korean"
  | "Japanese"
  | "Chinese"
  | "Spanish"
  | "French"
  | "German"
  | "Russian";

export type Theme = "light" | "dark";
export type TabId = "text" | "file" | "summary";
export type StreamTarget = TabId;
export type StreamStatus = "progress" | "output" | "done" | "error" | "cancelled";

export interface AiRequest {
  text: string;
  sourceLang?: Language;
  targetLang: Language;
  modelName: string;
  temperature: number;
  provider: Provider;
  baseUrl: string;
  apiKey?: string;
  chunkSize: number;
  originalFileName?: string;
}

export interface StreamPayload {
  taskId: string;
  target: StreamTarget;
  status: StreamStatus;
  output?: string;
  progress?: string;
  outputPath?: string;
  error?: string;
}

export interface FileTranslationResult {
  output: string;
  outputPath?: string;
}

export interface DroppedTextFile {
  fileName: string;
  text: string;
}
