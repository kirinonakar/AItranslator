#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};

const DEFAULT_LM_API_KEY: &str = "lm-studio";

#[derive(Default)]
struct AppState {
    cancelled_tasks: Mutex<HashSet<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiRequest {
    text: String,
    source_lang: Option<String>,
    target_lang: String,
    model_name: String,
    temperature: f32,
    provider: String,
    base_url: String,
    api_key: Option<String>,
    chunk_size: usize,
    original_file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamPayload {
    task_id: String,
    target: String,
    status: String,
    output: Option<String>,
    progress: Option<String>,
    output_path: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileTranslationResult {
    output: String,
    output_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextFilePayload {
    file_name: String,
    text: String,
}

enum Operation {
    Translate,
    Summarize,
}

#[tauri::command]
fn load_gemini_api_key() -> String {
    candidate_paths("gemini.txt")
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

#[tauri::command]
async fn fetch_lm_studio_models(base_url: String) -> Result<Vec<String>, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| error.to_string())?;

    let response = client
        .get(models_endpoint(&base_url))
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    let payload: Value = response.json().await.map_err(|error| error.to_string())?;
    let models = payload
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(models)
}

#[tauri::command]
async fn translate_text(
    task_id: String,
    target: String,
    request: AiRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    clear_cancelled(state.inner(), &task_id);
    run_streaming_chunks(
        app,
        state.inner(),
        task_id,
        target,
        request,
        Operation::Translate,
    )
    .await
}

#[tauri::command]
async fn summarize_text(
    task_id: String,
    target: String,
    request: AiRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    clear_cancelled(state.inner(), &task_id);
    run_streaming_chunks(
        app,
        state.inner(),
        task_id,
        target,
        request,
        Operation::Summarize,
    )
    .await
}

#[tauri::command]
async fn translate_file(
    task_id: String,
    target: String,
    request: AiRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<FileTranslationResult, String> {
    clear_cancelled(state.inner(), &task_id);

    if request.text.trim().is_empty() {
        let output = "Please upload a file first.".to_string();
        emit_stream(
            &app,
            StreamPayload {
                task_id,
                target,
                status: "error".to_string(),
                output: Some(output.clone()),
                progress: Some("No file content".to_string()),
                output_path: None,
                error: Some(output.clone()),
            },
        );
        return Ok(FileTranslationResult {
            output,
            output_path: None,
        });
    }

    let client = Client::new();
    let chunks = split_text_into_chunks(&request.text, request.chunk_size.max(1));
    let total_chunks = chunks.len();
    let mut full_translation = String::new();

    emit_stream(
        &app,
        StreamPayload {
            task_id: task_id.clone(),
            target: target.clone(),
            status: "progress".to_string(),
            output: Some(String::new()),
            progress: Some(format!("Starting translation of {total_chunks} chunks...")),
            output_path: None,
            error: None,
        },
    );

    for (index, chunk) in chunks.iter().enumerate() {
        if is_cancelled(state.inner(), &task_id) {
            clear_cancelled(state.inner(), &task_id);
            emit_stream(
                &app,
                StreamPayload {
                    task_id,
                    target,
                    status: "cancelled".to_string(),
                    output: Some(full_translation.clone()),
                    progress: Some("Cancelled".to_string()),
                    output_path: None,
                    error: None,
                },
            );
            return Ok(FileTranslationResult {
                output: full_translation,
                output_path: None,
            });
        }

        let prompt = create_formatted_prompt(
            request.source_lang.as_deref().unwrap_or("Auto Detect"),
            &request.target_lang,
            chunk,
        );

        emit_stream(
            &app,
            StreamPayload {
                task_id: task_id.clone(),
                target: target.clone(),
                status: "progress".to_string(),
                output: Some(full_translation.clone()),
                progress: Some(format!(
                    "Translating chunk {} of {total_chunks}...",
                    index + 1
                )),
                output_path: None,
                error: None,
            },
        );

        match post_chat_completion(&client, &request, &prompt, true).await {
            Ok(response) => {
                match read_streaming_response(
                    &app,
                    state.inner(),
                    &task_id,
                    &target,
                    response,
                    &full_translation,
                )
                .await
                {
                    Ok(StreamRead::Completed(chunk_translation)) => {
                        full_translation.push_str(&chunk_translation);
                        full_translation.push_str("\n\n");
                    }
                    Ok(StreamRead::Cancelled(partial)) => {
                        full_translation.push_str(&partial);
                        clear_cancelled(state.inner(), &task_id);
                        emit_stream(
                            &app,
                            StreamPayload {
                                task_id,
                                target,
                                status: "cancelled".to_string(),
                                output: Some(full_translation.clone()),
                                progress: Some("Cancelled".to_string()),
                                output_path: None,
                                error: None,
                            },
                        );
                        return Ok(FileTranslationResult {
                            output: full_translation,
                            output_path: None,
                        });
                    }
                    Err(error) => {
                        full_translation.push_str(&format!(
                            "\n[Error translating chunk {}: {}]\n",
                            index + 1,
                            error
                        ));
                    }
                }
            }
            Err(error) => {
                full_translation.push_str(&format!(
                    "\n[Error translating chunk {}: {}]\n",
                    index + 1,
                    error
                ));
            }
        }

        emit_stream(
            &app,
            StreamPayload {
                task_id: task_id.clone(),
                target: target.clone(),
                status: "output".to_string(),
                output: Some(full_translation.clone()),
                progress: Some(format!("Completed {} of {total_chunks} chunks.", index + 1)),
                output_path: None,
                error: None,
            },
        );
    }

    clear_cancelled(state.inner(), &task_id);
    let output_file_name = translated_file_name(request.original_file_name.as_deref());
    let output_path = write_text_to_temp(&full_translation, &output_file_name)?;

    emit_stream(
        &app,
        StreamPayload {
            task_id,
            target,
            status: "done".to_string(),
            output: Some(full_translation.clone()),
            progress: Some("Done".to_string()),
            output_path: Some(output_path.clone()),
            error: None,
        },
    );

    Ok(FileTranslationResult {
        output: full_translation,
        output_path: Some(output_path),
    })
}

#[tauri::command]
fn cancel_task(task_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .cancelled_tasks
        .lock()
        .map_err(|_| "Could not acquire cancellation lock".to_string())?
        .insert(task_id);
    Ok(())
}

#[tauri::command]
fn save_text_to_temp(text: String, file_name: String) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("Nothing to save".to_string());
    }

    write_text_to_temp(&text, &file_name)
}

#[tauri::command]
fn read_clipboard_text() -> Result<String, String> {
    #[cfg(windows)]
    {
        clipboard_win::get_clipboard_string().map_err(|error| error.to_string())
    }

    #[cfg(not(windows))]
    {
        Err("Clipboard paste is only supported in the Windows desktop app.".to_string())
    }
}

#[tauri::command]
fn write_clipboard_text(text: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        clipboard_win::set_clipboard_string(&text).map_err(|error| error.to_string())
    }

    #[cfg(not(windows))]
    {
        let _ = text;
        Err("Clipboard copy is only supported in the Windows desktop app.".to_string())
    }
}

#[tauri::command]
fn read_dropped_text_file(path: String) -> Result<TextFilePayload, String> {
    let file_path = PathBuf::from(path);
    let file_name = file_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("dropped.txt")
        .to_string();

    if !is_supported_text_file(&file_path) {
        return Err(format!("Unsupported file type: {file_name}"));
    }

    let text = fs::read_to_string(&file_path).map_err(|error| error.to_string())?;
    Ok(TextFilePayload { file_name, text })
}

async fn run_streaming_chunks(
    app: AppHandle,
    state: &AppState,
    task_id: String,
    target: String,
    request: AiRequest,
    operation: Operation,
) -> Result<String, String> {
    if request.text.trim().is_empty() {
        emit_stream(
            &app,
            StreamPayload {
                task_id,
                target,
                status: "done".to_string(),
                output: Some(String::new()),
                progress: Some(String::new()),
                output_path: None,
                error: None,
            },
        );
        return Ok(String::new());
    }

    let client = Client::new();
    let chunks = split_text_into_chunks(&request.text, request.chunk_size.max(1));
    let total_chunks = chunks.len();
    let mut full_output = String::new();

    for (index, chunk) in chunks.iter().enumerate() {
        if is_cancelled(state, &task_id) {
            clear_cancelled(state, &task_id);
            emit_stream(
                &app,
                StreamPayload {
                    task_id,
                    target,
                    status: "cancelled".to_string(),
                    output: Some(full_output.clone()),
                    progress: Some("Cancelled".to_string()),
                    output_path: None,
                    error: None,
                },
            );
            return Ok(full_output);
        }

        let progress = match operation {
            Operation::Translate => format!("Processing chunk {} of {total_chunks}...", index + 1),
            Operation::Summarize => format!("Summarizing chunk {} of {total_chunks}...", index + 1),
        };

        emit_stream(
            &app,
            StreamPayload {
                task_id: task_id.clone(),
                target: target.clone(),
                status: "progress".to_string(),
                output: Some(full_output.clone()),
                progress: Some(progress),
                output_path: None,
                error: None,
            },
        );

        let prompt = match operation {
            Operation::Translate => create_formatted_prompt(
                request.source_lang.as_deref().unwrap_or("Auto Detect"),
                &request.target_lang,
                chunk,
            ),
            Operation::Summarize => create_summary_prompt(chunk, &request.target_lang),
        };

        let response = match post_chat_completion(&client, &request, &prompt, true).await {
            Ok(response) => response,
            Err(error) => {
                let message = format_stream_error(&request, &operation, index + 1, &error);
                full_output.push_str(&message);
                emit_stream(
                    &app,
                    StreamPayload {
                        task_id,
                        target,
                        status: "error".to_string(),
                        output: Some(full_output.clone()),
                        progress: Some(format!("Error on chunk {}", index + 1)),
                        output_path: None,
                        error: Some(error),
                    },
                );
                return Ok(full_output);
            }
        };

        match read_streaming_response(&app, state, &task_id, &target, response, &full_output).await
        {
            Ok(StreamRead::Completed(chunk_output)) => {
                full_output.push_str(&chunk_output);
                full_output.push_str("\n\n");

                let progress = match operation {
                    Operation::Translate => {
                        format!("Completed {} of {total_chunks} chunks.", index + 1)
                    }
                    Operation::Summarize => format!(
                        "Completed summarizing {} of {total_chunks} chunks.",
                        index + 1
                    ),
                };

                emit_stream(
                    &app,
                    StreamPayload {
                        task_id: task_id.clone(),
                        target: target.clone(),
                        status: "output".to_string(),
                        output: Some(full_output.clone()),
                        progress: Some(progress),
                        output_path: None,
                        error: None,
                    },
                );
            }
            Ok(StreamRead::Cancelled(partial)) => {
                full_output.push_str(&partial);
                clear_cancelled(state, &task_id);
                emit_stream(
                    &app,
                    StreamPayload {
                        task_id,
                        target,
                        status: "cancelled".to_string(),
                        output: Some(full_output.clone()),
                        progress: Some("Cancelled".to_string()),
                        output_path: None,
                        error: None,
                    },
                );
                return Ok(full_output);
            }
            Err(error) => {
                let message = format_stream_error(&request, &operation, index + 1, &error);
                full_output.push_str(&message);
                emit_stream(
                    &app,
                    StreamPayload {
                        task_id,
                        target,
                        status: "error".to_string(),
                        output: Some(full_output.clone()),
                        progress: Some(format!("Error on chunk {}", index + 1)),
                        output_path: None,
                        error: Some(error),
                    },
                );
                return Ok(full_output);
            }
        }
    }

    clear_cancelled(state, &task_id);
    emit_stream(
        &app,
        StreamPayload {
            task_id,
            target,
            status: "done".to_string(),
            output: Some(full_output.clone()),
            progress: Some("Done".to_string()),
            output_path: None,
            error: None,
        },
    );

    Ok(full_output)
}

enum StreamRead {
    Completed(String),
    Cancelled(String),
}

async fn read_streaming_response(
    app: &AppHandle,
    state: &AppState,
    task_id: &str,
    target: &str,
    response: reqwest::Response,
    full_prefix: &str,
) -> Result<StreamRead, String> {
    let mut stream = response.bytes_stream();
    let mut line_buffer = String::new();
    let mut chunk_output = String::new();

    while let Some(next) = stream.next().await {
        if is_cancelled(state, task_id) {
            return Ok(StreamRead::Cancelled(chunk_output));
        }

        let bytes = next.map_err(|error| error.to_string())?;
        line_buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(newline_index) = line_buffer.find('\n') {
            let line: String = line_buffer.drain(..=newline_index).collect();
            if let Some(delta) = parse_sse_delta(&line)? {
                chunk_output.push_str(&delta);
                emit_stream(
                    app,
                    StreamPayload {
                        task_id: task_id.to_string(),
                        target: target.to_string(),
                        status: "output".to_string(),
                        output: Some(format!("{full_prefix}{chunk_output}")),
                        progress: None,
                        output_path: None,
                        error: None,
                    },
                );
            }
        }
    }

    if !line_buffer.trim().is_empty() {
        if let Some(delta) = parse_sse_delta(&line_buffer)? {
            chunk_output.push_str(&delta);
        }
    }

    Ok(StreamRead::Completed(chunk_output))
}

fn parse_sse_delta(line: &str) -> Result<Option<String>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return Ok(None);
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(None);
    };

    let payload = data.trim();
    if payload == "[DONE]" {
        return Ok(None);
    }

    let value: Value = serde_json::from_str(payload).map_err(|error| error.to_string())?;
    if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
        return Err(message.to_string());
    }

    Ok(value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned))
}

async fn post_chat_completion(
    client: &Client,
    request: &AiRequest,
    prompt: &str,
    stream: bool,
) -> Result<reqwest::Response, String> {
    let api_key = if request.provider == "Google" {
        request.api_key.clone().unwrap_or_default()
    } else {
        DEFAULT_LM_API_KEY.to_string()
    };

    let response = client
        .post(chat_endpoint(&request.base_url))
        .bearer_auth(api_key)
        .json(&json!({
            "model": request.model_name,
            "messages": [
                {
                    "role": "user",
                    "content": prompt,
                }
            ],
            "temperature": request.temperature,
            "stream": stream,
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(format!("HTTP {status}: {body}"))
}

fn create_formatted_prompt(source_lang: &str, target_lang: &str, text: &str) -> String {
    let source_code = language_code(source_lang);
    let target_code = language_code(target_lang);

    if source_lang == "Auto Detect" {
        return format!(
            "You are a professional translator. Identify the language of the following text and translate it into {target_lang} ({target_code}). Your goal is to accurately convey the meaning and nuances of the original text while adhering to {target_lang} grammar, vocabulary, and cultural sensitivities.\nProduce only the {target_lang} translation, without any additional explanations or commentary. Please translate the following text into {target_lang}:\n\n{text}"
        );
    }

    format!(
        "You are a professional {source_lang} ({source_code}) to {target_lang} ({target_code}) translator. Your goal is to accurately convey the meaning and nuances of the original {source_lang} text while adhering to {target_lang} grammar, vocabulary, and cultural sensitivities.\nProduce only the {target_lang} translation, without any additional explanations or commentary. Please translate the following {source_lang} text into {target_lang}:\n\n\n{text}"
    )
}

fn create_summary_prompt(text: &str, target_lang: &str) -> String {
    format!(
        "You are a professional assistant specializing in text summarization. Please summarize the following text into approximately 3 to 5 concise sentences in {target_lang}. The summary should capture the main points clearly and accurately.\nProvide only the summary without any introductory or concluding remarks.\n\nText to summarize:\n{text}"
    )
}

fn language_code(language: &str) -> &'static str {
    match language {
        "English" => "en",
        "Korean" => "ko",
        "Japanese" => "ja",
        "Chinese" => "zh",
        "Spanish" => "es",
        "French" => "fr",
        "German" => "de",
        "Russian" => "ru",
        _ => "auto",
    }
}

fn split_text_into_chunks(text: &str, max_chunk_size: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let max_chunk_size = max_chunk_size.max(1);
    let separators = ["\n\n", "\n", ". ", "? ", "! ", " "];
    let mut segments = vec![text.to_string()];

    for separator in separators {
        let mut new_segments = Vec::new();

        for segment in segments {
            if char_count(&segment) <= max_chunk_size {
                new_segments.push(segment);
                continue;
            }

            let parts = segment.split(separator).collect::<Vec<_>>();
            if parts.len() == 1 {
                new_segments.push(segment);
                continue;
            }

            for (index, part) in parts.iter().enumerate() {
                if index + 1 == parts.len() {
                    new_segments.push((*part).to_string());
                } else {
                    new_segments.push(format!("{part}{separator}"));
                }
            }
        }

        segments = new_segments;
    }

    let mut atoms = Vec::new();
    for segment in segments {
        if char_count(&segment) > max_chunk_size {
            atoms.extend(hard_split_chars(&segment, max_chunk_size));
        } else {
            atoms.push(segment);
        }
    }

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for atom in atoms {
        if char_count(&current_chunk) + char_count(&atom) <= max_chunk_size {
            current_chunk.push_str(&atom);
        } else {
            if !current_chunk.is_empty() {
                chunks.push(current_chunk);
            }
            current_chunk = atom;
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

fn hard_split_chars(text: &str, max_chunk_size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for character in text.chars() {
        if char_count(&current) >= max_chunk_size {
            chunks.push(current);
            current = String::new();
        }
        current.push(character);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn models_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/models") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/models")
    }
}

fn chat_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

fn format_stream_error(
    request: &AiRequest,
    operation: &Operation,
    chunk_number: usize,
    error: &str,
) -> String {
    let action = match operation {
        Operation::Translate => "translating",
        Operation::Summarize => "summarizing",
    };

    let context = if request.provider == "LM Studio" {
        format!(
            "Please ensure LM Studio is running and the server is started at {}.",
            request.base_url
        )
    } else {
        "Please ensure Google endpoint is correct and your API Key is valid.".to_string()
    };

    format!("\n[Error {action} chunk {chunk_number}: {error}]\n\n{context}")
}

fn get_output_dir() -> Result<PathBuf, String> {
    let base_dir = std::env::current_dir().map_err(|error| error.to_string())?;
    let output_dir = base_dir.join("output");
    if !output_dir.exists() {
        fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    }
    Ok(output_dir)
}

fn write_text_to_temp(text: &str, file_name: &str) -> Result<String, String> {
    let safe_name = sanitize_file_name(file_name);
    let output_dir = get_output_dir()?;
    let final_name = get_next_filename(&output_dir, &safe_name);
    let path = output_dir.join(final_name);
    fs::write(&path, text).map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

fn get_next_filename(output_dir: &Path, file_name: &str) -> String {
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("txt");

    let mut counter = 1;
    loop {
        let candidate = format!("{}{:03}.{}", stem, counter, extension);
        if !output_dir.join(&candidate).exists() {
            return candidate;
        }
        counter += 1;
        if counter > 999 {
            return format!("{}_{}.{}", stem, counter, extension);
        }
    }
}

#[tauri::command]
fn open_output_folder() -> Result<(), String> {
    let output_dir = get_output_dir()?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(output_dir)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn translated_file_name(original_file_name: Option<&str>) -> String {
    let original = original_file_name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("translation.txt");
    let safe = sanitize_file_name(original);
    let path = Path::new(&safe);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("txt");
    format!("{stem}_translated.{extension}")
}

fn is_supported_text_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("txt" | "md" | "py" | "js" | "html" | "json" | "csv")
    )
}

fn sanitize_file_name(file_name: &str) -> String {
    let sanitized = file_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();

    if sanitized.trim_matches('_').is_empty() {
        "output.txt".to_string()
    } else {
        sanitized
    }
}

fn candidate_paths(file_name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(current_dir) = std::env::current_dir() {
        paths.push(current_dir.join(file_name));
        if let Some(parent) = current_dir.parent() {
            paths.push(parent.join(file_name));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            paths.push(parent.join(file_name));
        }
    }

    paths
}

fn is_cancelled(state: &AppState, task_id: &str) -> bool {
    state
        .cancelled_tasks
        .lock()
        .map(|tasks| tasks.contains(task_id))
        .unwrap_or(false)
}

fn clear_cancelled(state: &AppState, task_id: &str) {
    if let Ok(mut tasks) = state.cancelled_tasks.lock() {
        tasks.remove(task_id);
    }
}

fn emit_stream(app: &AppHandle, payload: StreamPayload) {
    let _ = app.emit("ai-stream", payload);
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            load_gemini_api_key,
            fetch_lm_studio_models,
            translate_text,
            summarize_text,
            translate_file,
            cancel_task,
            save_text_to_temp,
            read_clipboard_text,
            write_clipboard_text,
            read_dropped_text_file,
            open_output_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
