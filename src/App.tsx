import { defaultWindowIcon } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { DragDropEvent } from "@tauri-apps/api/window";
import {
  Clipboard,
  Copy,
  Download,
  FileText,
  FolderOpen,
  Languages,
  Moon,
  RefreshCw,
  Square,
  RotateCcw,
  Sun,
  Upload,
  WandSparkles,
} from "lucide-react";
import { ChangeEvent, DragEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  CEREBRAS_BASE_URL,
  CEREBRAS_MODELS,
  DEFAULT_BASE_URL,
  DEFAULT_CEREBRAS_MODEL,
  DEFAULT_GOOGLE_MODEL,
  DEFAULT_LM_API_KEY,
  DEFAULT_OLLAMA_API_KEY,
  DEFAULT_OLLAMA_CLOUD_MODEL,
  DEFAULT_OLLAMA_MODEL,
  GOOGLE_BASE_URL,
  GOOGLE_MODELS,
  LM_STUDIO_MODELS,
  OLLAMA_BASE_URL,
  OLLAMA_CLOUD_BASE_URL,
  OLLAMA_CLOUD_MODELS,
  OLLAMA_MODELS,
  PROVIDERS,
  SOURCE_LANGUAGES,
  SUPPORTED_FILE_TYPES,
  TARGET_LANGUAGES,
} from "./constants";
import type { AiRequest, DroppedTextFile, FileTranslationResult, Language, Provider, StreamPayload, TabId, Theme } from "./types";

const clampNumber = (value: number, minimum: number, maximum: number) => {
  if (Number.isNaN(value)) return minimum;
  return Math.min(Math.max(value, minimum), maximum);
};
const readInitialChunkSize = (key: string, fallback: number, minimum: number, maximum: number) => {
  const saved = localStorage.getItem(key);
  if (saved === null) return fallback;
  const parsed = Number(saved);
  if (Number.isNaN(parsed)) return fallback;
  return clampNumber(parsed, minimum, maximum);
};

const createTaskId = (target: TabId) => {
  const randomPart = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
  return `${target}-${randomPart}`;
};

const readInitialTheme = (): Theme => {
  const saved = localStorage.getItem("theme");
  if (saved === "light" || saved === "dark") return saved;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
};

const getProviderBaseUrl = (provider: Provider) => {
  if (provider === "Google") return GOOGLE_BASE_URL;
  if (provider === "Cerebras") return CEREBRAS_BASE_URL;
  if (provider === "Ollama") return OLLAMA_BASE_URL;
  if (provider === "Ollama Cloud") return OLLAMA_CLOUD_BASE_URL;
  return DEFAULT_BASE_URL;
};

const getProviderModels = (provider: Provider) => {
  if (provider === "Google") return GOOGLE_MODELS;
  if (provider === "Cerebras") return CEREBRAS_MODELS;
  if (provider === "Ollama") return OLLAMA_MODELS;
  if (provider === "Ollama Cloud") return OLLAMA_CLOUD_MODELS;
  return LM_STUDIO_MODELS;
};

const getProviderDefaultModel = (provider: Provider) => {
  if (provider === "Google") return DEFAULT_GOOGLE_MODEL;
  if (provider === "Cerebras") return DEFAULT_CEREBRAS_MODEL;
  if (provider === "Ollama") return DEFAULT_OLLAMA_MODEL;
  if (provider === "Ollama Cloud") return DEFAULT_OLLAMA_CLOUD_MODEL;
  return LM_STUDIO_MODELS[0];
};

const includeSelectedModel = (models: string[], selectedModel: string) => {
  if (!selectedModel || models.includes(selectedModel)) return models;
  return [selectedModel, ...models];
};

const providerRequiresApiKey = (provider: Provider) => provider === "Google" || provider === "Cerebras" || provider === "Ollama Cloud";
const providerSupportsModelSync = (provider: Provider) => provider === "LM Studio" || provider === "Ollama" || provider === "Ollama Cloud";
const providerUsesCuratedModelList = (provider: Provider) => provider === "Google" || provider === "Cerebras";

const getStoredModel = (provider: Provider) => {
  const saved = localStorage.getItem(`modelName_${provider}`);
  const knownModels = getProviderModels(provider);
  if (saved && (!providerUsesCuratedModelList(provider) || knownModels.includes(saved))) return saved;

  const legacy = localStorage.getItem("modelName");
  if (legacy && (!providerUsesCuratedModelList(provider) || knownModels.includes(legacy))) return legacy;

  return getProviderDefaultModel(provider);
};

const getProviderDisplayModels = (provider: Provider) => includeSelectedModel(getProviderModels(provider), getStoredModel(provider));

const getProviderApiKeyCommands = (provider: Provider) => {
  if (provider === "Google") {
    return { load: "load_gemini_api_key", save: "save_gemini_api_key", label: "Google" };
  }
  if (provider === "Cerebras") {
    return { load: "load_cerebras_api_key", save: "save_cerebras_api_key", label: "Cerebras" };
  }
  if (provider === "Ollama Cloud") {
    return { load: "load_ollama_cloud_api_key", save: "save_ollama_cloud_api_key", label: "Ollama Cloud" };
  }
  return null;
};

const readInitialProvider = (): Provider => {
  const saved = localStorage.getItem("provider");
  if (!saved) return "LM Studio";
  const normalized = saved.trim().toLowerCase();
  if (normalized === "google") return "Google";
  if (normalized === "cerebras") return "Cerebras";
  if (normalized === "ollama") return "Ollama";
  if (normalized === "ollama cloud" || normalized === "ollamacloud") return "Ollama Cloud";
  if (normalized === "lm studio" || normalized === "lmstudio") return "LM Studio";
  return "LM Studio";
};

const readInitialModel = (initialProvider: Provider): string => {
  const saved = localStorage.getItem(`modelName_${initialProvider}`);
  if (saved) {
    const models = getProviderModels(initialProvider);
    if (!providerUsesCuratedModelList(initialProvider) || models.includes(saved)) return saved;
    return getProviderDefaultModel(initialProvider);
  }

  const legacySaved = localStorage.getItem("modelName");
  if (legacySaved) {
    const models = getProviderModels(initialProvider);
    if (!providerUsesCuratedModelList(initialProvider) || models.includes(legacySaved)) return legacySaved;
  }

  return getProviderDefaultModel(initialProvider);
};

const supportedFileExtensions = SUPPORTED_FILE_TYPES.split(",").map((extension) => extension.trim().toLowerCase());

const isSupportedTextFile = (file: File) => {
  const extension = `.${file.name.split(".").pop()?.toLowerCase() ?? ""}`;
  return supportedFileExtensions.includes(extension);
};

type TauriWindow = Window & {
  __TAURI_INTERNALS__?: {
    invoke?: unknown;
  };
};

const isTauriRuntime = () => typeof (window as TauriWindow).__TAURI_INTERNALS__?.invoke === "function";

const callTauri = async <T,>(command: string, args?: Record<string, unknown>) => {
  if (!isTauriRuntime()) {
    throw new Error("This action requires the Tauri desktop app. Run `npm run tauri:dev` to use it.");
  }

  return invoke<T>(command, args);
};

const normalizeApiKey = (value?: string) => value?.trim().replace(/^Bearer\s+/i, "").trim() ?? "";

const readClipboardText = () => {
  if (isTauriRuntime()) return callTauri<string>("read_clipboard_text");
  return navigator.clipboard.readText();
};

const writeClipboardText = (text: string) => {
  if (isTauriRuntime()) return callTauri<void>("write_clipboard_text", { text });
  return navigator.clipboard.writeText(text);
};

const extractModelNames = (payload: unknown) => {
  if (!payload || typeof payload !== "object") return [];
  const record = payload as {
    data?: Array<{ id?: string }>;
    models?: Array<{ name?: string; model?: string }>;
  };

  const openAiModels = record.data?.map((model) => model.id).filter((id): id is string => Boolean(id)) ?? [];
  if (openAiModels.length > 0) return openAiModels;

  return record.models
    ?.map((model) => model.name ?? model.model)
    .filter((name): name is string => Boolean(name)) ?? [];
};

const fetchProviderModels = async (baseUrl: string, apiKey?: string) => {
  const normalizedApiKey = normalizeApiKey(apiKey);
  if (isTauriRuntime()) {
    return callTauri<string[]>("fetch_provider_models", { baseUrl, apiKey: normalizedApiKey || null });
  }

  const params = new URLSearchParams({ baseUrl });
  if (normalizedApiKey) params.set("apiKey", normalizedApiKey);

  const response = await fetch(`/__provider_models?${params.toString()}`);
  if (!response.ok) {
    const body = await response.text().catch(() => "");
    throw new Error(`HTTP ${response.status}: ${body || response.statusText}`);
  }

  return extractModelNames(await response.json());
};

const languageCodes: Record<string, string> = {
  English: "en",
  Korean: "ko",
  Japanese: "ja",
  Chinese: "zh",
  Spanish: "es",
  French: "fr",
  German: "de",
  Russian: "ru",
};

const createFormattedPrompt = (sourceLang: Language, targetLang: Language, text: string) => {
  const sourceCode = languageCodes[sourceLang] ?? "auto";
  const targetCode = languageCodes[targetLang] ?? "en";

  if (sourceLang === "Auto Detect") {
    return (
      `You are a professional translator. ` +
      `Identify the language of the following text and translate it into ${targetLang} (${targetCode}). ` +
      `Your goal is to accurately convey the meaning and nuances of the original text while adhering to ${targetLang} grammar, vocabulary, and cultural sensitivities.\n` +
      `Produce only the ${targetLang} translation, without any additional explanations or commentary. Please translate the following text into ${targetLang}:\n\n` +
      text
    );
  }

  return (
    `You are a professional ${sourceLang} (${sourceCode}) to ${targetLang} (${targetCode}) translator. ` +
    `Your goal is to accurately convey the meaning and nuances of the original ${sourceLang} text while adhering to ${targetLang} grammar, vocabulary, and cultural sensitivities.\n` +
    `Produce only the ${targetLang} translation, without any additional explanations or commentary. Please translate the following ${sourceLang} text into ${targetLang}:\n\n\n` +
    text
  );
};

const createSummaryPrompt = (text: string, targetLang: Language) =>
  `You are a professional assistant specializing in text summarization. ` +
  `Please summarize the following text into approximately 3 to 5 concise sentences in ${targetLang}. ` +
  `The summary should capture the main points clearly and accurately.\n` +
  `Provide only the summary without any introductory or concluding remarks.\n\n` +
  `Text to summarize:\n${text}`;

const splitTextIntoChunks = (text: string, maxChunkSize: number) => {
  if (!text) return [];

  const safeMax = Math.max(1, maxChunkSize);
  const separators = ["\n\n", "\n", ". ", "? ", "! ", " "];
  let segments = [text];

  for (const separator of separators) {
    const nextSegments: string[] = [];

    for (const segment of segments) {
      if ([...segment].length <= safeMax) {
        nextSegments.push(segment);
        continue;
      }

      const parts = segment.split(separator);
      if (parts.length === 1) {
        nextSegments.push(segment);
        continue;
      }

      parts.forEach((part, index) => {
        nextSegments.push(index + 1 === parts.length ? part : `${part}${separator}`);
      });
    }

    segments = nextSegments;
  }

  const atoms = segments.flatMap((segment) => {
    const chars = [...segment];
    if (chars.length <= safeMax) return [segment];

    const hardChunks: string[] = [];
    for (let index = 0; index < chars.length; index += safeMax) {
      hardChunks.push(chars.slice(index, index + safeMax).join(""));
    }
    return hardChunks;
  });

  const chunks: string[] = [];
  let currentChunk = "";

  for (const atom of atoms) {
    if ([...currentChunk].length + [...atom].length <= safeMax) {
      currentChunk += atom;
    } else {
      if (currentChunk) chunks.push(currentChunk);
      currentChunk = atom;
    }
  }

  if (currentChunk) chunks.push(currentChunk);
  return chunks;
};

const postBrowserChatCompletion = async (request: AiRequest, prompt: string, stream: boolean, signal?: AbortSignal) => {
  const apiKey = providerRequiresApiKey(request.provider)
    ? normalizeApiKey(request.apiKey)
    : request.provider === "Ollama"
      ? DEFAULT_OLLAMA_API_KEY
      : DEFAULT_LM_API_KEY;
  const response = await fetch("/__chat_completions", {
    method: "POST",
    headers: {
      "content-type": "application/json",
    },
    body: JSON.stringify({
      baseUrl: request.baseUrl,
      apiKey,
      body: {
        model: request.modelName,
        messages: [{ role: "user", content: prompt }],
        temperature: request.temperature,
        stream,
      },
    }),
    signal,
  });

  if (!response.ok) {
    const body = await response.text().catch(() => "");
    throw new Error(`HTTP ${response.status}: ${body || response.statusText}`);
  }

  return response;
};

const parseStreamLine = (line: string) => {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith(":")) return "";

  const data = trimmed.startsWith("data:") ? trimmed.slice(5).trim() : "";
  if (!data || data === "[DONE]") return "";

  const payload = JSON.parse(data) as {
    choices?: Array<{ delta?: { content?: string } }>;
    error?: { message?: string };
  };

  if (payload.error?.message) throw new Error(payload.error.message);
  return payload.choices?.[0]?.delta?.content ?? "";
};

const readBrowserStream = async (
  response: Response,
  signal: AbortSignal,
  onDelta: (delta: string) => void | Promise<void>,
) => {
  if (!response.body) throw new Error("Streaming response is empty.");

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    if (signal.aborted) throw new DOMException("The operation was aborted.", "AbortError");

    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });

    let newlineIndex = buffer.indexOf("\n");
    while (newlineIndex >= 0) {
      const line = buffer.slice(0, newlineIndex);
      buffer = buffer.slice(newlineIndex + 1);
      const delta = parseStreamLine(line);
      if (delta) await onDelta(delta);
      newlineIndex = buffer.indexOf("\n");
    }
  }

  const rest = buffer.trim();
  if (rest) {
    const delta = parseStreamLine(rest);
    if (delta) await onDelta(delta);
  }
};

const pushLiveUpdate = (callback: () => void) => {
  callback();
  return new Promise<void>((resolve) => {
    requestAnimationFrame(() => resolve());
  });
};

const runBrowserTextOperation = async (
  request: AiRequest,
  operation: "translate" | "summarize",
  onUpdate: (output: string, progress: string) => void,
  signal: AbortSignal,
  onChunkStart?: (chunkIndex: number, accumulatedOutput: string) => void,
) => {
  if (!request.text.trim()) {
    onUpdate("", "");
    return "";
  }

  const chunks = splitTextIntoChunks(request.text, request.chunkSize);
  const totalChunks = chunks.length;
  const startIndex = request.startChunk ?? 0;
  let fullOutput = request.existingOutput ?? "";

  for (let index = startIndex; index < totalChunks; index++) {
    if (signal.aborted) throw new DOMException("The operation was aborted.", "AbortError");

    onChunkStart?.(index, fullOutput);

    const chunk = chunks[index];
    const progress =
      operation === "translate"
        ? `Processing chunk ${index + 1} of ${totalChunks}...`
        : `Summarizing chunk ${index + 1} of ${totalChunks}...`;
    const prompt =
      operation === "translate"
        ? createFormattedPrompt(request.sourceLang ?? "Auto Detect", request.targetLang, chunk)
        : createSummaryPrompt(chunk, request.targetLang);

    onUpdate(fullOutput, progress);
    const response = await postBrowserChatCompletion(request, prompt, true, signal);
    let chunkOutput = "";

    await readBrowserStream(response, signal, async (delta) => {
      chunkOutput += delta;
      await pushLiveUpdate(() => onUpdate(`${fullOutput}${chunkOutput}`, progress));
    });

    fullOutput += `${chunkOutput}\n\n`;
    const completedProgress =
      operation === "translate"
        ? `Completed ${index + 1} of ${totalChunks} chunks.`
        : `Completed summarizing ${index + 1} of ${totalChunks} chunks.`;
    onUpdate(fullOutput, completedProgress);
  }

  onUpdate(fullOutput, "Done");
  return fullOutput;
};

const runBrowserFileTranslation = async (
  request: AiRequest,
  onUpdate: (output: string, progress: string) => void,
  signal: AbortSignal,
  onChunkStart?: (chunkIndex: number, accumulatedOutput: string) => void,
): Promise<FileTranslationResult> => {
  if (!request.text.trim()) {
    const output = "Please upload a file first.";
    onUpdate(output, "No file content");
    return { output };
  }

  const chunks = splitTextIntoChunks(request.text, request.chunkSize);
  const totalChunks = chunks.length;
  const startIndex = request.startChunk ?? 0;
  let fullTranslation = request.existingOutput ?? "";
  onUpdate(fullTranslation, `Starting translation of ${totalChunks} chunks...`);

  for (let index = startIndex; index < totalChunks; index++) {
    if (signal.aborted) throw new DOMException("The operation was aborted.", "AbortError");

    onChunkStart?.(index, fullTranslation);

    const chunk = chunks[index];
    const prompt = createFormattedPrompt(request.sourceLang ?? "Auto Detect", request.targetLang, chunk);
    const progress = `Translating chunk ${index + 1} of ${totalChunks}...`;
    const response = await postBrowserChatCompletion(request, prompt, true, signal);
    let chunkTranslation = "";

    onUpdate(fullTranslation, progress);
    await readBrowserStream(response, signal, async (delta) => {
      chunkTranslation += delta;
      await pushLiveUpdate(() => onUpdate(`${fullTranslation}${chunkTranslation}`, progress));
    });

    fullTranslation += `${chunkTranslation}\n\n`;
    onUpdate(fullTranslation, `Completed ${index + 1} of ${totalChunks} chunks.`);
  }

  onUpdate(fullTranslation, "Done");
  return { output: fullTranslation };
};

const isAbortError = (error: unknown) =>
  error instanceof DOMException
    ? error.name === "AbortError"
    : error instanceof Error && error.name === "AbortError";

function App() {
  const [theme, setTheme] = useState<Theme>(readInitialTheme);
  
  const initialProvider = useMemo(() => readInitialProvider(), []);
  const [provider, setProvider] = useState<Provider>(initialProvider);
  const [apiKey, setApiKey] = useState("");
  const [hasLoadedApiKey, setHasLoadedApiKey] = useState(!isTauriRuntime());
  
  const [baseUrl, setBaseUrl] = useState(() => getProviderBaseUrl(initialProvider));
  const [models, setModels] = useState<string[]>(() => getProviderDisplayModels(initialProvider));
  const [googleModel, setGoogleModel] = useState<string>(() => getStoredModel("Google"));
  const [cerebrasModel, setCerebrasModel] = useState<string>(() => getStoredModel("Cerebras"));
  const [lmStudioModel, setLmStudioModel] = useState<string>(() => getStoredModel("LM Studio"));
  const [ollamaModel, setOllamaModel] = useState<string>(() => getStoredModel("Ollama"));
  const [ollamaCloudModel, setOllamaCloudModel] = useState<string>(() => getStoredModel("Ollama Cloud"));

  const modelName =
    provider === "Google"
      ? googleModel
      : provider === "Cerebras"
        ? cerebrasModel
        : provider === "Ollama"
          ? ollamaModel
          : provider === "Ollama Cloud"
            ? ollamaCloudModel
            : lmStudioModel;


  const [temperature, setTemperature] = useState(0.3);
  const [sourceLang, setSourceLang] = useState<Language>("Auto Detect");
  const [targetLang, setTargetLang] = useState<Language>("Korean");
  const [activeTab, setActiveTab] = useState<TabId>("text");
  const [notice, setNotice] = useState("Ready");
  const [isSyncingModels, setIsSyncingModels] = useState(false);

  const [inputText, setInputText] = useState("");
  const [outputText, setOutputText] = useState("");
  const [textChunkSize, setTextChunkSize] = useState(readInitialChunkSize("textChunkSize", 1500, 100, 5000));
  const [textProgress, setTextProgress] = useState("Ready");
  const [textPath, setTextPath] = useState("");
  const [isTextRunning, setIsTextRunning] = useState(false);

  const [fileName, setFileName] = useState("");
  const [fileContent, setFileContent] = useState("");
  const [filePreview, setFilePreview] = useState("");
  const [fileChunkSize, setFileChunkSize] = useState(readInitialChunkSize("fileChunkSize", 1500, 100, 5000));
  const [filePath, setFilePath] = useState("");
  const [fileProgress, setFileProgress] = useState("Ready");
  const [isFileRunning, setIsFileRunning] = useState(false);
  const [isDraggingFile, setIsDraggingFile] = useState(false);

  const [summaryInput, setSummaryInput] = useState("");
  const [summaryOutput, setSummaryOutput] = useState("");
  const [summaryChunkSize, setSummaryChunkSize] = useState(readInitialChunkSize("summaryChunkSize", 2000, 100, 100000));
  const [summaryProgress, setSummaryProgress] = useState("Ready");
  const [summaryPath, setSummaryPath] = useState("");
  const [isSummaryRunning, setIsSummaryRunning] = useState(false);

  const [canResumeText, setCanResumeText] = useState(false);
  const [canResumeFile, setCanResumeFile] = useState(false);
  const [canResumeSummary, setCanResumeSummary] = useState(false);

  const textTaskId = useRef("");
  const fileTaskId = useRef("");
  const summaryTaskId = useRef("");
  const browserTaskControllers = useRef<Partial<Record<TabId, AbortController>>>({});
  const fileInputRef = useRef<HTMLInputElement>(null);
  const savedApiKeyRef = useRef("");
  const currentKeyProviderRef = useRef<Provider | null>(null);
  const resumeInfo = useRef<Partial<Record<TabId, { startChunk: number; existingOutput: string; inputText: string; chunkSize: number }>>>({});
  const cleanOutputRef = useRef<{ text: string; file: string; summary: string }>({ text: "", file: "", summary: "" });

  const baseRequest = useMemo(
    () => ({
      targetLang,
      modelName,
      temperature,
      provider,
      baseUrl,
      apiKey,
    }),
    [apiKey, baseUrl, modelName, provider, targetLang, temperature],
  );

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("theme", theme);

    if (!isTauriRuntime()) return;
    void getCurrentWindow().setTheme(theme).catch(() => undefined);
  }, [theme]);

  useEffect(() => {
    localStorage.setItem("provider", provider);
  }, [provider]);

  useEffect(() => {
    localStorage.setItem("modelName_Google", googleModel);
    if (provider === "Google") {
      localStorage.setItem("modelName", googleModel);
    }
  }, [googleModel, provider]);

  useEffect(() => {
    localStorage.setItem("modelName_Cerebras", cerebrasModel);
    if (provider === "Cerebras") {
      localStorage.setItem("modelName", cerebrasModel);
    }
  }, [cerebrasModel, provider]);

  useEffect(() => {
    localStorage.setItem("modelName_LM Studio", lmStudioModel);
    if (provider === "LM Studio") {
      localStorage.setItem("modelName", lmStudioModel);
    }
  }, [lmStudioModel, provider]);

  useEffect(() => {
    localStorage.setItem("modelName_Ollama", ollamaModel);
    if (provider === "Ollama") {
      localStorage.setItem("modelName", ollamaModel);
    }
  }, [ollamaModel, provider]);

  useEffect(() => {
    localStorage.setItem("modelName_Ollama Cloud", ollamaCloudModel);
    if (provider === "Ollama Cloud") {
      localStorage.setItem("modelName", ollamaCloudModel);
    }
  }, [ollamaCloudModel, provider]);
  useEffect(() => {
    localStorage.setItem("textChunkSize", String(textChunkSize));
  }, [textChunkSize]);

  useEffect(() => {
    localStorage.setItem("fileChunkSize", String(fileChunkSize));
  }, [fileChunkSize]);

  useEffect(() => {
    localStorage.setItem("summaryChunkSize", String(summaryChunkSize));
  }, [summaryChunkSize]);


  useEffect(() => {
    if (!isTauriRuntime()) return;

    void defaultWindowIcon()
      .then((icon) => {
        if (!icon) return undefined;
        return getCurrentWindow().setIcon(icon);
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    const commands = getProviderApiKeyCommands(provider);
    if (!commands) {
      savedApiKeyRef.current = "";
      currentKeyProviderRef.current = provider;
      setApiKey("");
      setHasLoadedApiKey(true);
      return;
    }

    setHasLoadedApiKey(false);
    callTauri<string>(commands.load)
      .then((key) => {
        const normalized = normalizeApiKey(key);
        savedApiKeyRef.current = normalized;
        currentKeyProviderRef.current = provider;
        setApiKey(normalized);
      })
      .catch(() => {
        savedApiKeyRef.current = "";
        currentKeyProviderRef.current = provider;
        setApiKey("");
      })
      .finally(() => setHasLoadedApiKey(true));
  }, [provider]);

  useEffect(() => {
    if (!isTauriRuntime() || !hasLoadedApiKey) return;
    const commands = getProviderApiKeyCommands(provider);
    if (!commands) return;
    if (currentKeyProviderRef.current !== provider) return;

    const normalized = normalizeApiKey(apiKey);
    if (normalized === savedApiKeyRef.current) return;

    const timer = window.setTimeout(() => {
      callTauri<void>(commands.save, { apiKey: normalized })
        .then(() => {
          savedApiKeyRef.current = normalized;
          setNotice(normalized ? `${commands.label} API key saved to Windows Credential Manager.` : `${commands.label} API key removed from Windows Credential Manager.`);
        })
        .catch((error) => {
          setNotice(`API key save failed: ${String(error)}`);
        });
    }, 500);

    return () => window.clearTimeout(timer);
  }, [apiKey, hasLoadedApiKey, provider]);

  useEffect(() => {
    setBaseUrl(getProviderBaseUrl(provider));
    setModels(getProviderDisplayModels(provider));
    setNotice(`${provider} provider selected`);
  }, [provider]);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let unlisten: UnlistenFn | undefined;

    listen<StreamPayload>("ai-stream", ({ payload }) => {
      if (payload.target === "text" && payload.taskId === textTaskId.current) {
        if (payload.status === "progress" && payload.output !== undefined) {
          cleanOutputRef.current.text = payload.output;
        }
        if (payload.output !== undefined) setOutputText(payload.output);
        if (payload.progress) setTextProgress(payload.progress);
        if (payload.outputPath) setTextPath(payload.outputPath);
        if (["done", "error", "cancelled"].includes(payload.status)) {
          setIsTextRunning(false);
          if (["error", "cancelled"].includes(payload.status) && payload.completedChunks !== undefined && payload.completedChunks > 0) {
            resumeInfo.current.text = {
              startChunk: payload.completedChunks,
              existingOutput: cleanOutputRef.current.text,
              inputText,
              chunkSize: clampNumber(textChunkSize, 100, 5000),
            };
            setCanResumeText(true);
          } else {
            resumeInfo.current.text = undefined;
            setCanResumeText(false);
          }
        }
      }

      if (payload.target === "file" && payload.taskId === fileTaskId.current) {
        if (payload.status === "progress" && payload.output !== undefined) {
          cleanOutputRef.current.file = payload.output;
        }
        if (payload.output !== undefined) setFilePreview(payload.output);
        if (payload.progress) setFileProgress(payload.progress);
        if (payload.outputPath) setFilePath(payload.outputPath);
        if (["done", "error", "cancelled"].includes(payload.status)) {
          setIsFileRunning(false);
          if (["error", "cancelled"].includes(payload.status) && payload.completedChunks !== undefined && payload.completedChunks > 0) {
            resumeInfo.current.file = {
              startChunk: payload.completedChunks,
              existingOutput: cleanOutputRef.current.file,
              inputText: fileContent,
              chunkSize: clampNumber(fileChunkSize, 100, 5000),
            };
            setCanResumeFile(true);
          } else {
            resumeInfo.current.file = undefined;
            setCanResumeFile(false);
          }
        }
      }

      if (payload.target === "summary" && payload.taskId === summaryTaskId.current) {
        if (payload.status === "progress" && payload.output !== undefined) {
          cleanOutputRef.current.summary = payload.output;
        }
        if (payload.output !== undefined) setSummaryOutput(payload.output);
        if (payload.progress) setSummaryProgress(payload.progress);
        if (payload.outputPath) setSummaryPath(payload.outputPath);
        if (["done", "error", "cancelled"].includes(payload.status)) {
          setIsSummaryRunning(false);
          if (["error", "cancelled"].includes(payload.status) && payload.completedChunks !== undefined && payload.completedChunks > 0) {
            resumeInfo.current.summary = {
              startChunk: payload.completedChunks,
              existingOutput: cleanOutputRef.current.summary,
              inputText: summaryInput,
              chunkSize: clampNumber(summaryChunkSize, 100, 100000),
            };
            setCanResumeSummary(true);
          } else {
            resumeInfo.current.summary = undefined;
            setCanResumeSummary(false);
          }
        }
      }

      if (payload.error) setNotice(payload.error);
    }).then((handler) => {
      unlisten = handler;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  const syncModels = useCallback(async (options?: { automatic?: boolean }) => {
    if (!providerSupportsModelSync(provider)) return;

    setIsSyncingModels(true);
    setNotice(options?.automatic ? `Auto-syncing ${provider} models...` : `Fetching models from ${provider}...`);
    try {
      const fetchedModels = await fetchProviderModels(baseUrl, provider === "Ollama Cloud" ? apiKey : undefined);
      if (fetchedModels.length === 0) {
        setNotice(`Could not fetch models from ${provider}.`);
        return;
      }

      setModels(fetchedModels);
      if (provider === "Ollama") {
        setOllamaModel((prev) => (fetchedModels.includes(prev) ? prev : fetchedModels[0]));
      } else if (provider === "Ollama Cloud") {
        setOllamaCloudModel((prev) => (fetchedModels.includes(prev) ? prev : fetchedModels[0]));
      } else {
        setLmStudioModel((prev) => (fetchedModels.includes(prev) ? prev : fetchedModels[0]));
      }
      setNotice(`Fetched ${fetchedModels.length} model(s).`);
    } catch (error) {
      setNotice(`${options?.automatic ? "Auto sync" : "Model sync"} failed: ${String(error)}`);
    } finally {
      setIsSyncingModels(false);
    }
  }, [apiKey, baseUrl, provider]);

  useEffect(() => {
    if (!providerSupportsModelSync(provider) || !hasLoadedApiKey) return;

    const timer = window.setTimeout(() => {
      void syncModels({ automatic: true });
    }, 350);

    return () => window.clearTimeout(timer);
  }, [baseUrl, hasLoadedApiKey, provider, syncModels]);

  const buildTextRequest = (text: string, chunkSize: number, source?: Language, originalFileName?: string): AiRequest => ({
    ...baseRequest,
    text,
    sourceLang: source,
    chunkSize,
    originalFileName,
  });

  const createBrowserAbortController = (target: TabId) => {
    browserTaskControllers.current[target]?.abort();
    const controller = new AbortController();
    browserTaskControllers.current[target] = controller;
    return controller;
  };

  const clearBrowserAbortController = (target: TabId) => {
    delete browserTaskControllers.current[target];
  };

  const translateText = async (resume?: { startChunk: number; existingOutput: string }) => {
    const taskId = createTaskId("text");
    textTaskId.current = taskId;
    setCanResumeText(false);
    if (!resume) {
      setTextPath("");
      setOutputText("");
      cleanOutputRef.current.text = "";
    }
    setTextProgress(resume ? `Resuming from chunk ${resume.startChunk + 1}...` : "Starting translation...");
    setIsTextRunning(true);

    try {
      const request = buildTextRequest(inputText, clampNumber(textChunkSize, 100, 5000), sourceLang);
      if (resume) {
        request.startChunk = resume.startChunk;
        request.existingOutput = resume.existingOutput;
      }
      const finalText = isTauriRuntime()
        ? await callTauri<string>("translate_text", { taskId, target: "text", request })
        : await runBrowserTextOperation(
            request,
            "translate",
            (output, progress) => {
              setOutputText(output);
              setTextProgress(progress);
            },
            createBrowserAbortController("text").signal,
            (chunkIndex, accumulatedOutput) => {
              cleanOutputRef.current.text = accumulatedOutput;
              resumeInfo.current.text = {
                startChunk: chunkIndex,
                existingOutput: accumulatedOutput,
                inputText,
                chunkSize: clampNumber(textChunkSize, 100, 5000),
              };
            },
          );
      setOutputText(finalText);
      resumeInfo.current.text = undefined;
      setCanResumeText(false);
    } catch (error) {
      if (isAbortError(error)) {
        setTextProgress("Cancelled");
        setNotice("Translation cancelled.");
        if (resumeInfo.current.text && resumeInfo.current.text.startChunk > 0) setCanResumeText(true);
      } else {
        setTextProgress("Error");
        setNotice(`Translation failed: ${String(error)}`);
        if (resumeInfo.current.text && resumeInfo.current.text.startChunk > 0) setCanResumeText(true);
      }
    } finally {
      setIsTextRunning(false);
      clearBrowserAbortController("text");
    }
  };

  const summarizeText = async (resume?: { startChunk: number; existingOutput: string }) => {
    const taskId = createTaskId("summary");
    summaryTaskId.current = taskId;
    setCanResumeSummary(false);
    if (!resume) {
      setSummaryPath("");
      setSummaryOutput("");
      cleanOutputRef.current.summary = "";
    }
    setSummaryProgress(resume ? `Resuming from chunk ${resume.startChunk + 1}...` : "Starting summary...");
    setIsSummaryRunning(true);

    try {
      const request = buildTextRequest(summaryInput, clampNumber(summaryChunkSize, 100, 100000));
      if (resume) {
        request.startChunk = resume.startChunk;
        request.existingOutput = resume.existingOutput;
      }
      const finalSummary = isTauriRuntime()
        ? await callTauri<string>("summarize_text", { taskId, target: "summary", request })
        : await runBrowserTextOperation(
            request,
            "summarize",
            (output, progress) => {
              setSummaryOutput(output);
              setSummaryProgress(progress);
            },
            createBrowserAbortController("summary").signal,
            (chunkIndex, accumulatedOutput) => {
              cleanOutputRef.current.summary = accumulatedOutput;
              resumeInfo.current.summary = {
                startChunk: chunkIndex,
                existingOutput: accumulatedOutput,
                inputText: summaryInput,
                chunkSize: clampNumber(summaryChunkSize, 100, 100000),
              };
            },
          );
      setSummaryOutput(finalSummary);
      resumeInfo.current.summary = undefined;
      setCanResumeSummary(false);
    } catch (error) {
      if (isAbortError(error)) {
        setSummaryProgress("Cancelled");
        setNotice("Summary cancelled.");
        if (resumeInfo.current.summary && resumeInfo.current.summary.startChunk > 0) setCanResumeSummary(true);
      } else {
        setSummaryProgress("Error");
        setNotice(`Summary failed: ${String(error)}`);
        if (resumeInfo.current.summary && resumeInfo.current.summary.startChunk > 0) setCanResumeSummary(true);
      }
    } finally {
      setIsSummaryRunning(false);
      clearBrowserAbortController("summary");
    }
  };

  const translateFile = async (resume?: { startChunk: number; existingOutput: string }) => {
    const taskId = createTaskId("file");
    fileTaskId.current = taskId;
    setCanResumeFile(false);
    if (!resume) {
      setFilePath("");
      setFilePreview("");
      cleanOutputRef.current.file = "";
    }
    setFileProgress(resume ? `Resuming from chunk ${resume.startChunk + 1}...` : "Starting file translation...");
    setIsFileRunning(true);

    try {
      const request = buildTextRequest(fileContent, clampNumber(fileChunkSize, 100, 5000), sourceLang, fileName);
      if (resume) {
        request.startChunk = resume.startChunk;
        request.existingOutput = resume.existingOutput;
      }
      const result = isTauriRuntime()
        ? await callTauri<FileTranslationResult>("translate_file", { taskId, target: "file", request })
        : await runBrowserFileTranslation(
            request,
            (output, progress) => {
              setFilePreview(output);
              setFileProgress(progress);
            },
            createBrowserAbortController("file").signal,
            (chunkIndex, accumulatedOutput) => {
              cleanOutputRef.current.file = accumulatedOutput;
              resumeInfo.current.file = {
                startChunk: chunkIndex,
                existingOutput: accumulatedOutput,
                inputText: fileContent,
                chunkSize: clampNumber(fileChunkSize, 100, 5000),
              };
            },
          );
      setFilePreview(result.output);
      if (result.outputPath) setFilePath(result.outputPath);
      resumeInfo.current.file = undefined;
      setCanResumeFile(false);
    } catch (error) {
      if (isAbortError(error)) {
        setFileProgress("Cancelled");
        setNotice("File translation cancelled.");
        if (resumeInfo.current.file && resumeInfo.current.file.startChunk > 0) setCanResumeFile(true);
      } else {
        setFileProgress("Error");
        setNotice(`File translation failed: ${String(error)}`);
        if (resumeInfo.current.file && resumeInfo.current.file.startChunk > 0) setCanResumeFile(true);
      }
    } finally {
      setIsFileRunning(false);
      clearBrowserAbortController("file");
    }
  };

  const cancelTask = async (target: TabId) => {
    const taskId = target === "text" ? textTaskId.current : target === "file" ? fileTaskId.current : summaryTaskId.current;
    if (!taskId) return;

    if (target === "text") setTextProgress("Stopping...");
    if (target === "file") setFileProgress("Stopping...");
    if (target === "summary") setSummaryProgress("Stopping...");

    if (isTauriRuntime()) {
      await callTauri("cancel_task", { taskId });
      return;
    }

    browserTaskControllers.current[target]?.abort();
  };

  const resumeText = () => {
    const info = resumeInfo.current.text;
    if (!info) return;
    setOutputText(info.existingOutput);
    setTextProgress("Resuming...");
    setNotice("Resuming translation...");
    void translateText({ startChunk: info.startChunk, existingOutput: info.existingOutput });
  };

  const resumeFile = () => {
    const info = resumeInfo.current.file;
    if (!info) return;
    setFilePreview(info.existingOutput);
    setFileProgress("Resuming...");
    setNotice("Resuming file translation...");
    void translateFile({ startChunk: info.startChunk, existingOutput: info.existingOutput });
  };

  const resumeSummary = () => {
    const info = resumeInfo.current.summary;
    if (!info) return;
    setSummaryOutput(info.existingOutput);
    setSummaryProgress("Resuming...");
    setNotice("Resuming summary...");
    void summarizeText({ startChunk: info.startChunk, existingOutput: info.existingOutput });
  };

  const saveText = async (text: string, fileNameForSave: string, onSaved: (path: string) => void) => {
    if (!text.trim()) {
      setNotice("Nothing to save yet.");
      return;
    }

    try {
      if (!isTauriRuntime()) {
        const url = URL.createObjectURL(new Blob([text], { type: "text/plain;charset=utf-8" }));
        const link = document.createElement("a");
        link.href = url;
        link.download = fileNameForSave;
        link.click();
        URL.revokeObjectURL(url);
        onSaved(fileNameForSave);
        setNotice(`Prepared ${fileNameForSave} for download.`);
        return;
      }

      const path = await callTauri<string>("save_text_to_temp", { text, fileName: fileNameForSave });
      onSaved(path);
      setNotice(`Saved to ${path}`);
    } catch (error) {
      setNotice(`Save failed: ${String(error)}`);
    }
  };

  const copyText = async (text: string) => {
    if (!text) {
      setNotice("Nothing to copy yet.");
      return;
    }

    try {
      await writeClipboardText(text);
      setNotice("Copied to clipboard.");
    } catch (error) {
      setNotice(`Clipboard copy failed: ${String(error)}`);
    }
  };

  const pasteText = async (target: "text" | "summary") => {
    try {
      const text = await readClipboardText();
      if (target === "text") setInputText(text);
      if (target === "summary") setSummaryInput(text);
      setNotice("Pasted from clipboard.");
    } catch (error) {
      setNotice(`Clipboard paste failed: ${String(error)}`);
    }
  };

  const applyLoadedTextFile = useCallback((name: string, text: string) => {
    setFileName(name);
    setFileContent(text);
    setFilePreview("");
    setFilePath("");
    setFileProgress(`${name} loaded (${text.length.toLocaleString()} characters).`);
    setNotice(`Loaded ${name}.`);
  }, []);

  const loadTextFile = useCallback(async (file: File) => {
    if (!file) return;

    if (!isSupportedTextFile(file)) {
      setFileProgress("Unsupported file type");
      setNotice(`Unsupported file type: ${file.name}`);
      return;
    }

    try {
      const text = await file.text();
      applyLoadedTextFile(file.name, text);
    } catch (error) {
      setFileProgress("File read error");
      setNotice(`Could not read file: ${String(error)}`);
    }
  }, [applyLoadedTextFile]);

  const loadDroppedTextFilePath = useCallback(async (path: string) => {
    try {
      const droppedFile = await callTauri<DroppedTextFile>("read_dropped_text_file", { path });
      applyLoadedTextFile(droppedFile.fileName, droppedFile.text);
    } catch (error) {
      setFileProgress("File read error");
      setNotice(`Could not read dropped file: ${String(error)}`);
    }
  }, [applyLoadedTextFile]);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let unlisten: UnlistenFn | undefined;

    const updateDragState = (payload: DragDropEvent) => {
      if (activeTab !== "file") return;

      if (payload.type === "enter" || payload.type === "over") {
        setIsDraggingFile(true);
        return;
      }

      setIsDraggingFile(false);

      if (payload.type === "drop") {
        const droppedPath = payload.paths[0];
        if (droppedPath) void loadDroppedTextFilePath(droppedPath);
      }
    };

    getCurrentWindow()
      .onDragDropEvent(({ payload }) => updateDragState(payload))
      .then((handler) => {
        unlisten = handler;
      })
      .catch(() => undefined);

    return () => {
      unlisten?.();
    };
  }, [activeTab, loadDroppedTextFilePath]);

  const onFileSelected = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;

    await loadTextFile(file);
    event.currentTarget.value = "";
  };

  const onFileDragOver = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
    setIsDraggingFile(true);
  };

  const onFileDragLeave = (event: DragEvent<HTMLDivElement>) => {
    const nextTarget = event.relatedTarget;
    if (!(nextTarget instanceof Node) || !event.currentTarget.contains(nextTarget)) {
      setIsDraggingFile(false);
    }
  };

  const onFileDrop = async (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setIsDraggingFile(false);

    const file = event.dataTransfer.files?.[0];
    if (!file) return;

    await loadTextFile(file);
  };

  const isKeyRequired = providerRequiresApiKey(provider);

  return (
    <div className="app-shell">
      <main className="workspace">
        <div className="left-rail">
          <header className="topbar">
            <div className="brand-lockup">
              <div className="brand-mark">
                <Languages size={22} strokeWidth={2.4} />
              </div>
              <div>
                <h1>AI Universal Translator</h1>
                <p>LM Studio, Ollama, Google and Cerebras compatible desktop translator</p>
              </div>
            </div>
          </header>

          <aside className="settings-panel">
            <section className="panel-section">
              <div className="section-heading">
                <span>API Settings</span>
                <div className="settings-heading-actions">
                  <button className="icon-button compact" type="button" onClick={() => setTheme(theme === "light" ? "dark" : "light")} aria-label="Toggle theme">
                    {theme === "light" ? <Moon size={17} /> : <Sun size={17} />}
                  </button>
                </div>
              </div>

              <label className="field">
                <div className="field-heading">
                  <span>Provider</span>
                  <span className="status-chip provider-status">{notice}</span>
                </div>
                <select value={provider} onChange={(event) => setProvider(event.target.value as Provider)}>
                  {PROVIDERS.map((item) => (
                    <option key={item} value={item}>
                      {item}
                    </option>
                  ))}
                </select>
              </label>

              {isKeyRequired && (
                <label className="field">
                  <span>{provider} API Key</span>
                  <input value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={`Enter ${provider} API Key here...`} type="password" />
                  <small>The API key is stored in Windows Credential Manager</small>
                </label>
              )}

              <label className="field">
                <span>Server URL</span>
                <input value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} disabled={isKeyRequired} />
                <small>API endpoint address</small>
              </label>

              <label className="field">
                <span>Model Name</span>
                <div className="inline-field">
                  <select value={modelName} onChange={(event) => {
                    const val = event.target.value;
                    if (provider === "Google") {
                      setGoogleModel(val);
                    } else if (provider === "Cerebras") {
                      setCerebrasModel(val);
                    } else if (provider === "Ollama") {
                      setOllamaModel(val);
                    } else if (provider === "Ollama Cloud") {
                      setOllamaCloudModel(val);
                    } else {
                      setLmStudioModel(val);
                    }
                  }}>
                    {models.map((model) => (
                      <option value={model} key={model}>
                        {model}
                      </option>
                    ))}
                  </select>
                  {providerSupportsModelSync(provider) && (
                    <button className="icon-button compact" type="button" onClick={() => void syncModels()} disabled={isSyncingModels} aria-label="Sync models">
                      <RefreshCw size={17} className={isSyncingModels ? "spinning" : ""} />
                    </button>
                  )}
                </div>
                <small>{models.length} model{models.length === 1 ? "" : "s"} available.</small>
              </label>

              <label className="field">
                <span>Temperature</span>
                <div className="range-row">
                  <input
                    type="range"
                    min="0"
                    max="1"
                    step="0.1"
                    value={temperature}
                    onChange={(event) => setTemperature(Number(event.target.value))}
                  />
                  <output>{temperature.toFixed(1)}</output>
                </div>
                <small>낮을수록 더 정확합니다.</small>
              </label>
            </section>

            <section className="panel-section">
              <div className="section-heading">
                <span>Languages</span>
              </div>
              <label className="field">
                <span>Source Language</span>
                <select value={sourceLang} onChange={(event) => setSourceLang(event.target.value as Language)}>
                  {SOURCE_LANGUAGES.map((language) => (
                    <option key={language}>{language}</option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>Target Language</span>
                <select value={targetLang} onChange={(event) => setTargetLang(event.target.value as Language)}>
                  {TARGET_LANGUAGES.map((language) => (
                    <option key={language}>{language}</option>
                  ))}
                </select>
              </label>
            </section>
          </aside>
        </div>

        <section className="tool-panel">
          <nav className="tabs" aria-label="Translation modes">
            <TabButton active={activeTab === "text"} onClick={() => setActiveTab("text")} icon={<Languages size={17} />} label="Text Translation" />
            <TabButton active={activeTab === "file"} onClick={() => setActiveTab("file")} icon={<FileText size={17} />} label="File Translation" />
            <TabButton active={activeTab === "summary"} onClick={() => setActiveTab("summary")} icon={<WandSparkles size={17} />} label="Text Summary" />
          </nav>

          {activeTab === "text" && (
            <ModePanel
              inputLabel="Input Text"
              outputLabel="Translation"
              inputPlaceholder="번역할 내용을 입력하세요... (Enter text to translate...)"
              inputValue={inputText}
              outputValue={outputText}
              onInputChange={setInputText}
              onOutputChange={setOutputText}
              chunkSize={textChunkSize}
              onChunkSizeChange={setTextChunkSize}
              chunkMax={5000}
              progress={textProgress}
              isRunning={isTextRunning}
              primaryLabel="Translate Text"
              onPrimary={translateText}
              onStop={() => cancelTask("text")}
              onPaste={() => pasteText("text")}
              onCopy={() => copyText(outputText)}
              onSave={() => saveText(outputText, "translated.txt", setTextPath)}
              savedPath={textPath}
              canResume={canResumeText}
              onResume={resumeText}
            />
          )}

          {activeTab === "file" && (
            <div className="mode-panel">
              <div
                className={`file-drop${isDraggingFile ? " is-dragging" : ""}`}
                onDragOver={onFileDragOver}
                onDragLeave={onFileDragLeave}
                onDrop={onFileDrop}
              >
                <div className="file-drop-icon">
                  <Upload size={22} />
                </div>
                <div>
                  <span className="file-drop-title">Upload Text File</span>
                  <p>Drag and drop a UTF-8 text file here, or browse from your computer.</p>
                  <small>Supported: .txt, .md, .py, .js, .html, .json, .csv</small>
                </div>
                <input
                  ref={fileInputRef}
                  id="file-input"
                  className="file-input-hidden"
                  type="file"
                  accept={SUPPORTED_FILE_TYPES}
                  onChange={onFileSelected}
                />
                <button className="browse-button" type="button" onClick={() => fileInputRef.current?.click()}>
                  <FolderOpen size={17} />
                  Browse
                </button>
              </div>

              <div className="file-meta">
                <span>{fileName || "No file selected"}</span>
                <span>{fileContent ? `${fileContent.length.toLocaleString()} characters` : "Waiting for text file"}</span>
              </div>

              <label className="field fill">
                <span>Translation Preview</span>
                <textarea spellCheck="false" value={filePreview} onChange={(event) => setFilePreview(event.target.value)} />
              </label>

              <div className="control-grid">
                <label className="field">
                  <span>Chunk Size (Characters)</span>
                  <input
                    type="number"
                    min="100"
                    max="5000"
                    value={fileChunkSize}
                    onChange={(event) => setFileChunkSize(Number(event.target.value))}
                  />
                  <small>Adjust chunk size to fit model context window.</small>
                </label>
                <label className="field">
                  <span>Progress</span>
                  <input value={fileProgress} readOnly />
                </label>
              </div>

              <div className="action-row">
                {canResumeFile ? (
                  <button className="primary-button" type="button" onClick={resumeFile} disabled={isFileRunning || !fileContent}>
                    <RotateCcw size={17} />
                    Resume
                  </button>
                ) : (
                  <button className="primary-button" type="button" onClick={() => translateFile()} disabled={isFileRunning || !fileContent}>
                    <Languages size={17} />
                    Translate File
                  </button>
                )}
                <button className="secondary-button" type="button" onClick={() => cancelTask("file")} disabled={!isFileRunning}>
                  <Square size={15} />
                  Stop
                </button>
                <button className="secondary-button" type="button" onClick={() => copyText(filePreview)} disabled={!filePreview}>
                  <Copy size={16} />
                  Copy Preview
                </button>
                <button className="secondary-button" type="button" onClick={() => void callTauri("open_output_folder")}>
                  <FolderOpen size={16} />
                  Open Folder
                </button>
              </div>

              {filePath && <PathBanner label="Translated file" path={filePath} />}
            </div>
          )}

          {activeTab === "summary" && (
            <ModePanel
              inputLabel="Input Text"
              outputLabel="Summary Result"
              inputPlaceholder="요약할 내용을 입력하세요... (Enter text to summarize...)"
              inputValue={summaryInput}
              outputValue={summaryOutput}
              onInputChange={setSummaryInput}
              onOutputChange={setSummaryOutput}
              chunkSize={summaryChunkSize}
              onChunkSizeChange={setSummaryChunkSize}
              chunkMax={100000}
              progress={summaryProgress}
              isRunning={isSummaryRunning}
              primaryLabel="Summarize Text"
              onPrimary={summarizeText}
              onStop={() => cancelTask("summary")}
              onPaste={() => pasteText("summary")}
              onCopy={() => copyText(summaryOutput)}
              onSave={() => saveText(summaryOutput, "summary.txt", setSummaryPath)}
              savedPath={summaryPath}
              canResume={canResumeSummary}
              onResume={resumeSummary}
            />
          )}
        </section>
      </main>
    </div>
  );
}

interface TabButtonProps {
  active: boolean;
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}

function TabButton({ active, icon, label, onClick }: TabButtonProps) {
  return (
    <button type="button" className={active ? "active" : ""} onClick={onClick}>
      {icon}
      {label}
    </button>
  );
}

interface ModePanelProps {
  inputLabel: string;
  outputLabel: string;
  inputPlaceholder: string;
  inputValue: string;
  outputValue: string;
  onInputChange: (value: string) => void;
  onOutputChange: (value: string) => void;
  chunkSize: number;
  onChunkSizeChange: (value: number) => void;
  chunkMax: number;
  progress: string;
  isRunning: boolean;
  primaryLabel: string;
  onPrimary: () => void;
  onStop: () => void;
  onPaste: () => void;
  onCopy: () => void;
  onSave: () => void;
  savedPath: string;
  canResume?: boolean;
  onResume?: () => void;
}

function ModePanel({
  inputLabel,
  outputLabel,
  inputPlaceholder,
  inputValue,
  outputValue,
  onInputChange,
  onOutputChange,
  chunkSize,
  onChunkSizeChange,
  chunkMax,
  progress,
  isRunning,
  primaryLabel,
  onPrimary,
  onStop,
  onPaste,
  onCopy,
  onSave,
  savedPath,
  canResume,
  onResume,
}: ModePanelProps) {
  return (
    <div className="mode-panel">
      <div className="text-grid">
        <label className="field fill">
          <span>{inputLabel}</span>
          <textarea spellCheck="false" value={inputValue} onChange={(event) => onInputChange(event.target.value)} placeholder={inputPlaceholder} />
        </label>
        <label className="field fill">
          <span>{outputLabel}</span>
          <textarea spellCheck="false" value={outputValue} onChange={(event) => onOutputChange(event.target.value)} />
        </label>
      </div>

      <div className="control-grid">
        <label className="field">
          <span>Chunk Size (Characters)</span>
          <input type="number" min="100" max={chunkMax} value={chunkSize} onChange={(event) => onChunkSizeChange(Number(event.target.value))} />
          <small>Adjust chunk size for long text.</small>
        </label>
        <label className="field">
          <span>Progress</span>
          <input value={progress} readOnly />
        </label>
      </div>

      <div className="action-row">
        <button className="secondary-button" type="button" onClick={onPaste}>
          <Clipboard size={16} />
          Paste Input
        </button>
        {canResume && onResume ? (
          <button className="primary-button" type="button" onClick={onResume} disabled={isRunning || !inputValue.trim()}>
            <RotateCcw size={17} />
            Resume
          </button>
        ) : (
          <button className="primary-button" type="button" onClick={onPrimary} disabled={isRunning || !inputValue.trim()}>
            <Languages size={17} />
            {primaryLabel}
          </button>
        )}
        <button className="secondary-button" type="button" onClick={onStop} disabled={!isRunning}>
          <Square size={15} />
          Stop
        </button>
        <button className="secondary-button" type="button" onClick={onSave} disabled={!outputValue.trim()}>
          <Download size={16} />
          Save Result
        </button>
        <button className="secondary-button" type="button" onClick={onCopy} disabled={!outputValue.trim()}>
          <Copy size={16} />
          Copy
        </button>
        <button className="secondary-button" type="button" onClick={() => void callTauri("open_output_folder")}>
          <FolderOpen size={16} />
          Open Folder
        </button>
      </div>

      {savedPath && <PathBanner label="Saved file" path={savedPath} />}
    </div>
  );
}

function PathBanner({ label, path }: { label: string; path: string }) {
  return (
    <div className="path-banner">
      <span>{label}</span>
      <code>{path}</code>
    </div>
  );
}

export default App;
