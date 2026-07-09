# AI Universal Translator

AI Universal Translator is a desktop translation and summarization app built with Tauri, Rust, TypeScript, and React.

The app supports OpenAI-compatible chat completion APIs via local providers (LM Studio, Ollama) and cloud providers (Google Gemini, Cerebras, Ollama Cloud).

![UI Preview](screenshot.png)

## Features

- Text translation with streaming output
- Text summarization with streaming output
- UTF-8 text file translation
- Automatic chunking for long text and large files
- Multiple provider support: Local (LM Studio, Ollama) & Cloud (Google Gemini, Cerebras, Ollama Cloud)
- Local model list auto-sync (LM Studio, Ollama, Ollama Cloud)
- Secure API key storage via Windows Credential Manager for cloud providers
- Automatic API key migration from `gemini.txt` for Google Gemini
- Source language auto-detection option
- Adjustable temperature and chunk size
- Stop/cancel controls for running tasks
- Copy, paste, and save result actions
- Automatic sequential file naming (e.g., `translated001.txt`)
- Open local `./output` folder directly from the app
- Light and dark theme toggle

## Tech Stack

- Tauri 2
- Rust
- React
- TypeScript
- Vite
- lucide-react

## Requirements

- Node.js 20 or newer
- npm
- Rust toolchain
- A running local LLM provider or cloud provider API key

### 📥 Download
You can download the latest version from the [Releases Page](https://github.com/kirinonakar/AItranslator/releases).

## Manual Build

Install dependencies:

```bash
npm install
```

Run the Tauri desktop app in development mode:

```bash
npm run tauri:dev
```

Run only the Vite frontend for UI development:

```bash
npm run dev
```

Build the frontend:

```bash
npm run build
```

Build the desktop app:

```bash
npm run tauri:build
```

## Provider Setup

### 💻 Local Providers (No API Key)

* **LM Studio**: Run local server at `http://localhost:1234/v1`. Click model sync to fetch loaded models.
* **Ollama**: Run local server at `http://localhost:11434/v1`. Click model sync to fetch downloaded models.

### ☁️ Cloud Providers (API Key required)
*API keys are securely stored in Windows Credential Manager.*

* **Google Gemini**: Uses `https://generativelanguage.googleapis.com/v1beta/openai/`. Supports direct key input or auto-import from `gemini.txt` at the root.
* **Cerebras**: Uses `https://api.cerebras.ai/v1`.
* **Ollama Cloud**: Uses `https://ollama.com/v1`. Supports model sync.

## Supported Languages

Source languages:

- Auto Detect
- English
- Korean
- Japanese
- Chinese (Simplified)
- Chinese (Traditional)
- Spanish
- French
- German
- Russian

Target languages:

- English
- Korean
- Japanese
- Chinese (Simplified)
- Chinese (Traditional)
- Spanish
- French
- German
- Russian

## Supported File Types

The file translation view accepts UTF-8 text-based files:

- `.txt`
- `.md`
- `.py`
- `.js`
- `.html`
- `.json`
- `.csv`

Translated and saved files are stored in the `./output` directory within the project folder. The app automatically increments filenames (e.g., `filename001.txt`, `filename002.txt`) to prevent overwriting existing results.

## Project Structure

```text
.
├── index.html             # Vite entry HTML
├── output/                # Translated and saved files
├── package.json           # Frontend and Tauri scripts
├── src/                   # React + TypeScript frontend
│   ├── App.tsx
│   ├── constants.ts
│   ├── main.tsx
│   ├── styles.css
│   └── types.ts
└── src-tauri/             # Tauri + Rust backend
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json
    ├── capabilities/
    ├── icons/
    └── src/
        └── main.rs
```

## Development Notes

The Rust backend owns the API calls, streaming response parsing, chunk splitting, cancellation state, and temporary file writes. The React frontend owns the application state, provider controls, tabs, theme toggle, clipboard interactions, and result display.

Streaming responses are emitted from Rust to the frontend through Tauri events.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

