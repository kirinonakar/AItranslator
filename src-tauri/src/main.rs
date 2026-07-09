#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures_util::StreamExt;
use regex::Regex;
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

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_NOT_FOUND},
    Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_MAX_CREDENTIAL_BLOB_SIZE,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    },
};

const DEFAULT_LM_API_KEY: &str = "lm-studio";
const DEFAULT_OLLAMA_API_KEY: &str = "ollama";
const GOOGLE_CREDENTIAL_TARGET: &str = "AI Universal Translator: Google API Key";
const LEGACY_GOOGLE_CREDENTIAL_TARGET: &str = "AI Universal Translator: Google API Bearer Token";
const GOOGLE_CREDENTIAL_USER: &str = "API Key";
const CEREBRAS_CREDENTIAL_TARGET: &str = "AI Universal Translator: Cerebras API Key";
const CEREBRAS_CREDENTIAL_USER: &str = "API Key";
const OLLAMA_CLOUD_CREDENTIAL_TARGET: &str = "AI Universal Translator: Ollama Cloud API Key";
const OLLAMA_CLOUD_CREDENTIAL_USER: &str = "API Key";
const BEARER_PREFIX: &str = "Bearer ";

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
    start_chunk: Option<usize>,
    existing_output: Option<String>,
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
    completed_chunks: Option<usize>,
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
fn load_gemini_api_key() -> Result<String, String> {
    match read_google_api_key_from_credential_manager() {
        Ok(Some(api_key)) => Ok(api_key),
        Ok(None) => {
            let api_key = normalize_google_api_key(&load_gemini_api_key_from_file());
            if !api_key.is_empty() {
                let _ = save_google_api_key_to_credential_manager(&api_key);
            }
            Ok(api_key)
        }
        Err(error) => {
            let api_key = normalize_google_api_key(&load_gemini_api_key_from_file());
            if api_key.is_empty() {
                Err(error)
            } else {
                Ok(api_key)
            }
        }
    }
}

#[tauri::command]
fn save_gemini_api_key(api_key: String) -> Result<(), String> {
    save_google_api_key_to_credential_manager(&api_key)
}

#[cfg(windows)]
#[tauri::command]
fn load_cerebras_api_key() -> Result<String, String> {
    match read_credential_api_key(CEREBRAS_CREDENTIAL_TARGET) {
        Ok(Some(api_key)) => Ok(api_key),
        Ok(None) => Ok(String::new()),
        Err(error) => Err(error),
    }
}

#[cfg(not(windows))]
#[tauri::command]
fn load_cerebras_api_key() -> Result<String, String> {
    Ok(String::new())
}

#[cfg(windows)]
#[tauri::command]
fn save_cerebras_api_key(api_key: String) -> Result<(), String> {
    let api_key = normalize_google_api_key(&api_key);
    if api_key.is_empty() {
        delete_credential(CEREBRAS_CREDENTIAL_TARGET)
    } else {
        let mut secret = api_key.into_bytes();
        if secret.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err("Cerebras API key is too long for Windows Credential Manager.".to_string());
        }

        let mut target_name = to_wide(CEREBRAS_CREDENTIAL_TARGET);
        let mut user_name = to_wide(CEREBRAS_CREDENTIAL_USER);
        let mut comment = to_wide("Cerebras API key for AI Universal Translator");
        let mut credential = CREDENTIALW::default();
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target_name.as_mut_ptr();
        credential.Comment = comment.as_mut_ptr();
        credential.CredentialBlobSize = secret.len() as u32;
        credential.CredentialBlob = secret.as_mut_ptr();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        credential.UserName = user_name.as_mut_ptr();

        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            Err(credential_error("write", unsafe { GetLastError() }))
        } else {
            Ok(())
        }
    }
}

#[cfg(not(windows))]
#[tauri::command]
fn save_cerebras_api_key(api_key: String) -> Result<(), String> {
    if normalize_google_api_key(&api_key).is_empty() {
        Ok(())
    } else {
        Err("Windows Credential Manager is only available on Windows.".to_string())
    }
}

#[cfg(windows)]
#[tauri::command]
fn load_ollama_cloud_api_key() -> Result<String, String> {
    match read_credential_api_key(OLLAMA_CLOUD_CREDENTIAL_TARGET) {
        Ok(Some(api_key)) => Ok(api_key),
        Ok(None) => Ok(String::new()),
        Err(error) => Err(error),
    }
}

#[cfg(not(windows))]
#[tauri::command]
fn load_ollama_cloud_api_key() -> Result<String, String> {
    Ok(String::new())
}

#[cfg(windows)]
#[tauri::command]
fn save_ollama_cloud_api_key(api_key: String) -> Result<(), String> {
    let api_key = normalize_google_api_key(&api_key);
    if api_key.is_empty() {
        delete_credential(OLLAMA_CLOUD_CREDENTIAL_TARGET)
    } else {
        let mut secret = api_key.into_bytes();
        if secret.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err(
                "Ollama Cloud API key is too long for Windows Credential Manager.".to_string(),
            );
        }

        let mut target_name = to_wide(OLLAMA_CLOUD_CREDENTIAL_TARGET);
        let mut user_name = to_wide(OLLAMA_CLOUD_CREDENTIAL_USER);
        let mut comment = to_wide("Ollama Cloud API key for AI Universal Translator");
        let mut credential = CREDENTIALW::default();
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target_name.as_mut_ptr();
        credential.Comment = comment.as_mut_ptr();
        credential.CredentialBlobSize = secret.len() as u32;
        credential.CredentialBlob = secret.as_mut_ptr();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        credential.UserName = user_name.as_mut_ptr();

        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            Err(credential_error("write", unsafe { GetLastError() }))
        } else {
            Ok(())
        }
    }
}

#[cfg(not(windows))]
#[tauri::command]
fn save_ollama_cloud_api_key(api_key: String) -> Result<(), String> {
    if normalize_google_api_key(&api_key).is_empty() {
        Ok(())
    } else {
        Err("Windows Credential Manager is only available on Windows.".to_string())
    }
}

fn load_gemini_api_key_from_file() -> String {
    candidate_paths("gemini.txt")
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn normalize_google_api_key(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed == "\"\"" || trimmed == "''" {
        return String::new();
    }

    match trimmed.get(..BEARER_PREFIX.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(BEARER_PREFIX) => trimmed
            .get(BEARER_PREFIX.len()..)
            .unwrap_or_default()
            .trim()
            .to_string(),
        _ => trimmed.to_string(),
    }
}

fn credential_error(action: &str, error_code: u32) -> String {
    format!("Windows Credential Manager {action} failed with error code {error_code}.")
}

#[cfg(windows)]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn read_google_api_key_from_credential_manager() -> Result<Option<String>, String> {
    if let Some(api_key) = read_credential_api_key(GOOGLE_CREDENTIAL_TARGET)? {
        return Ok(Some(api_key));
    }

    if let Some(api_key) = read_credential_api_key(LEGACY_GOOGLE_CREDENTIAL_TARGET)? {
        let _ = save_google_api_key_to_credential_manager(&api_key);
        let _ = delete_credential(LEGACY_GOOGLE_CREDENTIAL_TARGET);
        return Ok(Some(api_key));
    }

    Ok(None)
}

#[cfg(windows)]
fn read_credential_api_key(target: &str) -> Result<Option<String>, String> {
    let raw_key = match read_credential_secret(target)? {
        Some(value) => value,
        None => return Ok(None),
    };

    let api_key = normalize_google_api_key(&raw_key);
    if api_key.is_empty() {
        let _ = delete_credential(target);
        Ok(None)
    } else {
        Ok(Some(api_key))
    }
}

#[cfg(windows)]
fn read_credential_secret(target: &str) -> Result<Option<String>, String> {
    let target_name = to_wide(target);
    let mut credential_ptr: *mut CREDENTIALW = std::ptr::null_mut();

    let ok = unsafe {
        CredReadW(
            target_name.as_ptr(),
            CRED_TYPE_GENERIC,
            0,
            &mut credential_ptr,
        )
    };

    if ok == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_NOT_FOUND {
            return Ok(None);
        }
        return Err(credential_error("read", error));
    }

    if credential_ptr.is_null() {
        return Ok(None);
    }

    let credential = unsafe { &*credential_ptr };
    let has_blob = credential.CredentialBlobSize > 0 && !credential.CredentialBlob.is_null();
    let secret = if !has_blob {
        String::new()
    } else {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                credential.CredentialBlob as *const u8,
                credential.CredentialBlobSize as usize,
            )
        };
        String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_string()
    };

    unsafe {
        CredFree(credential_ptr.cast());
    }

    if secret.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret))
    }
}

#[cfg(not(windows))]
fn read_google_api_key_from_credential_manager() -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(windows)]
fn save_google_api_key_to_credential_manager(api_key: &str) -> Result<(), String> {
    let api_key = normalize_google_api_key(api_key);
    if api_key.is_empty() {
        return delete_google_api_key_from_credential_manager();
    }

    let mut secret = api_key.into_bytes();
    if secret.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
        return Err("Google API key is too long for Windows Credential Manager.".to_string());
    }

    let mut target_name = to_wide(GOOGLE_CREDENTIAL_TARGET);
    let mut user_name = to_wide(GOOGLE_CREDENTIAL_USER);
    let mut comment = to_wide("Google API key for AI Universal Translator");
    let mut credential = CREDENTIALW::default();
    credential.Type = CRED_TYPE_GENERIC;
    credential.TargetName = target_name.as_mut_ptr();
    credential.Comment = comment.as_mut_ptr();
    credential.CredentialBlobSize = secret.len() as u32;
    credential.CredentialBlob = secret.as_mut_ptr();
    credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
    credential.UserName = user_name.as_mut_ptr();

    let ok = unsafe { CredWriteW(&credential, 0) };
    if ok == 0 {
        Err(credential_error("write", unsafe { GetLastError() }))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn save_google_api_key_to_credential_manager(api_key: &str) -> Result<(), String> {
    if normalize_google_api_key(api_key).is_empty() {
        Ok(())
    } else {
        Err("Windows Credential Manager is only available on Windows.".to_string())
    }
}

#[cfg(windows)]
fn delete_google_api_key_from_credential_manager() -> Result<(), String> {
    delete_credential(GOOGLE_CREDENTIAL_TARGET)?;
    delete_credential(LEGACY_GOOGLE_CREDENTIAL_TARGET)
}

#[cfg(windows)]
fn delete_credential(target: &str) -> Result<(), String> {
    let target_name = to_wide(target);
    let ok = unsafe { CredDeleteW(target_name.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_NOT_FOUND {
            Ok(())
        } else {
            Err(credential_error("delete", error))
        }
    } else {
        Ok(())
    }
}

#[tauri::command]
async fn fetch_provider_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| error.to_string())?;

    let mut request = client.get(models_endpoint(&base_url));
    let api_key = api_key
        .as_deref()
        .map(normalize_google_api_key)
        .unwrap_or_default();
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    let payload: Value = response.json().await.map_err(|error| error.to_string())?;
    Ok(parse_model_names(&payload))
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
                completed_chunks: None,
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
    let start_index = request.start_chunk.unwrap_or(0).min(total_chunks);
    let mut full_translation = request.existing_output.clone().unwrap_or_default();

    emit_stream(
        &app,
        StreamPayload {
            task_id: task_id.clone(),
            target: target.clone(),
            status: "progress".to_string(),
            output: Some(full_translation.clone()),
            progress: Some(format!("Starting translation of {total_chunks} chunks...")),
            output_path: None,
            error: None,
            completed_chunks: Some(start_index),
        },
    );

    for (index, chunk) in chunks.iter().enumerate().skip(start_index) {
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
                    completed_chunks: Some(index),
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
                completed_chunks: None,
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
                                completed_chunks: Some(index),
                            },
                        );
                        return Ok(FileTranslationResult {
                            output: full_translation,
                            output_path: None,
                        });
                    }
                    Err(error) => {
                        emit_stream(
                            &app,
                            StreamPayload {
                                task_id,
                                target,
                                status: "error".to_string(),
                                output: Some(full_translation.clone()),
                                progress: Some(format!("Error on chunk {}", index + 1)),
                                output_path: None,
                                error: Some(error),
                                completed_chunks: Some(index),
                            },
                        );
                        return Ok(FileTranslationResult {
                            output: full_translation,
                            output_path: None,
                        });
                    }
                }
            }
            Err(error) => {
                emit_stream(
                    &app,
                    StreamPayload {
                        task_id,
                        target,
                        status: "error".to_string(),
                        output: Some(full_translation.clone()),
                        progress: Some(format!("Error on chunk {}", index + 1)),
                        output_path: None,
                        error: Some(error),
                        completed_chunks: Some(index),
                    },
                );
                return Ok(FileTranslationResult {
                    output: full_translation,
                    output_path: None,
                });
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
                completed_chunks: None,
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
            completed_chunks: Some(total_chunks),
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
                completed_chunks: None,
            },
        );
        return Ok(String::new());
    }

    let client = Client::new();
    let chunks = split_text_into_chunks(&request.text, request.chunk_size.max(1));
    let total_chunks = chunks.len();
    let start_index = request.start_chunk.unwrap_or(0).min(total_chunks);
    let mut full_output = request.existing_output.clone().unwrap_or_default();

    for (index, chunk) in chunks.iter().enumerate().skip(start_index) {
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
                    completed_chunks: Some(index),
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
                completed_chunks: None,
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
                emit_stream(
                    &app,
                    StreamPayload {
                        task_id,
                        target,
                        status: "error".to_string(),
                        output: Some(full_output.clone()),
                        progress: Some(format!("Error on chunk {}", index + 1)),
                        output_path: None,
                        error: Some(message),
                        completed_chunks: Some(index),
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
                        completed_chunks: None,
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
                        completed_chunks: Some(index),
                    },
                );
                return Ok(full_output);
            }
            Err(error) => {
                let message = format_stream_error(&request, &operation, index + 1, &error);
                emit_stream(
                    &app,
                    StreamPayload {
                        task_id,
                        target,
                        status: "error".to_string(),
                        output: Some(full_output.clone()),
                        progress: Some(format!("Error on chunk {}", index + 1)),
                        output_path: None,
                        error: Some(message),
                        completed_chunks: Some(index),
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
            completed_chunks: Some(total_chunks),
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
                        output: Some(format!("{full_prefix}{}", filter_thoughts(&chunk_output))),
                        progress: None,
                        output_path: None,
                        error: None,
                        completed_chunks: None,
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

    Ok(StreamRead::Completed(filter_thoughts(&chunk_output)))
}

fn filter_thoughts(text: &str) -> String {
    // Rust's regex crate does not support backreferences (\1).
    // We must match each tag pair explicitly.

    // 1. Remove closed blocks: <thought>...</thought> or <think>...</think>
    let closed_re = Regex::new(r"(?s)<thought>.*?</thought>|<think>.*?</think>").unwrap();
    let intermediate = closed_re.replace_all(text, "");

    // 2. Remove unclosed blocks (active thinking): <thought>... or <think>... until end of string
    let unclosed_re = Regex::new(r"(?s)<thought>.*$|<think>.*$").unwrap();
    let final_output = unclosed_re.replace_all(&intermediate, "");

    final_output.trim().to_string()
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
    let api_key = if request.provider.eq_ignore_ascii_case("Google") {
        let request_api_key = request
            .api_key
            .as_deref()
            .map(normalize_google_api_key)
            .unwrap_or_default();
        if request_api_key.is_empty() {
            read_google_api_key_from_credential_manager().map(|key| key.unwrap_or_default())?
        } else {
            request_api_key
        }
    } else if request.provider.eq_ignore_ascii_case("Cerebras") {
        let request_api_key = request
            .api_key
            .as_deref()
            .map(normalize_google_api_key)
            .unwrap_or_default();
        if request_api_key.is_empty() {
            read_credential_api_key(CEREBRAS_CREDENTIAL_TARGET)
                .map(|key| key.unwrap_or_default())?
        } else {
            request_api_key
        }
    } else if request.provider.eq_ignore_ascii_case("Ollama Cloud")
        || request.provider.eq_ignore_ascii_case("OllamaCloud")
    {
        let request_api_key = request
            .api_key
            .as_deref()
            .map(normalize_google_api_key)
            .unwrap_or_default();
        if request_api_key.is_empty() {
            read_credential_api_key(OLLAMA_CLOUD_CREDENTIAL_TARGET)
                .map(|key| key.unwrap_or_default())?
        } else {
            request_api_key
        }
    } else if request.provider.eq_ignore_ascii_case("Ollama") {
        DEFAULT_OLLAMA_API_KEY.to_string()
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
        "Chinese (Simplified)" => "zh-Hans",
        "Chinese (Traditional)" => "zh-Hant",
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

fn parse_model_names(payload: &Value) -> Vec<String> {
    let openai_models = payload
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

    if !openai_models.is_empty() {
        return openai_models;
    }

    payload
        .get("models")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("name")
                        .or_else(|| item.get("model"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
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

    let context = if request.provider.eq_ignore_ascii_case("LM Studio")
        || request.provider.eq_ignore_ascii_case("lmstudio")
    {
        format!(
            "Please ensure LM Studio is running and the server is started at {}.",
            request.base_url
        )
    } else if request.provider.eq_ignore_ascii_case("Ollama") {
        format!(
            "Please ensure Ollama is running and the OpenAI-compatible server is available at {}.",
            request.base_url
        )
    } else if request.provider.eq_ignore_ascii_case("Ollama Cloud")
        || request.provider.eq_ignore_ascii_case("OllamaCloud")
    {
        "Please ensure Ollama Cloud endpoint is correct and your API Key is valid.".to_string()
    } else if request.provider.eq_ignore_ascii_case("Cerebras") {
        "Please ensure Cerebras endpoint is correct and your API Key is valid.".to_string()
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
            save_gemini_api_key,
            load_cerebras_api_key,
            save_cerebras_api_key,
            load_ollama_cloud_api_key,
            save_ollama_cloud_api_key,
            fetch_provider_models,
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
