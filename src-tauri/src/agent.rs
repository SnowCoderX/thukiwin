use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::{DirEntry, WalkDir};

const MAX_TOOL_OUTPUT_CHARS: usize = 8_000;
const MAX_FILE_PREVIEW_CHARS: usize = 10_000;
const MAX_FILE_SCAN_BYTES: u64 = 512 * 1024;
const MAX_SEARCH_RESULTS: usize = 20;
const MAX_VISITED_ENTRIES: usize = 20_000;

#[derive(Debug, Deserialize)]
pub struct SearchFilesArgs {
    pub query: String,
    #[serde(default)]
    pub search_content: bool,
    #[serde(default)]
    pub root: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReadFilePreviewArgs {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTextFileArgs {
    pub filename: String,
    pub content: String,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Deserialize)]
pub struct OpenItemArgs {
    pub target: String,
    #[serde(default)]
    pub arguments: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
}

#[derive(Debug, Deserialize)]
pub struct FetchUrlPreviewArgs {
    pub url: String,
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Search local files by name or by text content. Use for finding notes, configs, documents, or project files on disk.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Text to search for in the file name or file contents."
                        },
                        "search_content": {
                            "type": "boolean",
                            "description": "When true, search inside text files instead of only matching file names."
                        },
                        "root": {
                            "type": "string",
                            "description": "Optional absolute root directory to search. Defaults to the user's home directory."
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_file_preview",
                "description": "Read a local text file preview by path or file name. Returns a truncated preview for large files.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path or file name to the file to preview. Absolute paths are preferred, but desktop/documents/downloads aliases and plain file names also work."
                        }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Read the current local date and time from this computer. Use for questions about what time or date it is on the user's PC right now."
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_processes",
                "description": "List currently running processes on this computer."
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_text_file",
                "description": "Create a plain text file. Use destination='desktop' to save on the user's Desktop, or pass an absolute directory path.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "filename": {
                            "type": "string",
                            "description": "File name such as notes.txt or joke.txt. If no extension is provided, .txt will be added."
                        },
                        "content": {
                            "type": "string",
                            "description": "UTF-8 text content to write into the file."
                        },
                        "destination": {
                            "type": "string",
                            "description": "Use 'desktop' for the Desktop, or provide an absolute target directory path."
                        },
                        "overwrite": {
                            "type": "boolean",
                            "description": "When true, replace an existing file with the same name."
                        }
                    },
                    "required": ["filename", "content"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the web for current information and return the top result snippets with URLs. Use this for weather, current events, websites, products, and any question that needs fresh online data.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query text."
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "fetch_url_preview",
                "description": "Fetch a web page and return a readable text preview. Use it after web_search when you need details from a specific result.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "Absolute URL to fetch."
                        }
                    },
                    "required": ["url"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "open_item",
                "description": "Open a file, folder, URL, or application on this computer. Use it when the user explicitly asks to open, launch, start, or show something, not when you need to read web content.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "target": {
                            "type": "string",
                            "description": "File path, folder path, URL, desktop/documents/downloads alias, file name, or application name such as notepad or explorer."
                        },
                        "arguments": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional command-line arguments to pass when launching an application."
                        }
                    },
                    "required": ["target"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_system_info",
                "description": "Return basic OS, machine, and user environment information."
            }
        }),
    ]
}

pub fn tool_system_prompt(safe_mode: bool) -> String {
    let safe_mode_line = if safe_mode {
        "Safe mode is ON. Inspection tools are allowed. You may open files, folders, URLs, and applications when the user explicitly asks. You may create a new plain text file on the Desktop only when the user explicitly asks, but do not overwrite files or write elsewhere."
    } else {
        "Safe mode is OFF. You may inspect the computer, open items, and create plain text files when the user asks."
    };

    format!(
        concat!(
            "You are allowed to use local desktop tools when they help answer the user's request.\n",
            "{safe_mode_line}\n",
            "Additional profile-specific instructions may appear above. Follow them as part of your role.\n",
            "Rules:\n",
            "- Use tools only when they materially improve the answer.\n",
            "- Prefer concise tool usage and avoid repeating the same search.\n",
            "- Never claim you changed the computer unless a write tool actually succeeded.\n",
            "- Match the user's language unless they explicitly ask for another language.\n",
            "- Give the answer in exactly one language. Never append a translation, gloss, or restated copy of the answer in another language unless the user explicitly asked for a translation.\n",
            "- When creating a text file, keep the file content in the user's language unless asked otherwise.\n",
            "- If a tool already returned a file path earlier in the chat, reuse that exact path for follow-up file actions.\n",
            "- For fresh or current information from the internet, use web_search and then fetch_url_preview when needed. Do not answer current-web questions from memory.\n",
            "- For the current local time or date on the user's computer, use get_current_time. Do not guess.\n",
            "- If the user asks what a web page or browser search result says, open_item is not enough. Use web_search or fetch_url_preview to actually read the content.\n",
            "- If the user asks to open, launch, or show a local file, folder, URL, or app, use the open_item tool instead of saying you cannot control the interface.\n",
            "- For plain text files, prefer opening them in Notepad by default.\n",
            "- Treat open_item as a launch request only. Do not claim you visually confirmed that a window appeared.\n",
            "- If a tool fails, explain the failure briefly and continue if possible.\n",
            "- After using tools, answer normally in plain language.\n"
        ),
        safe_mode_line = safe_mode_line
    )
}

pub fn summarize_tool_args(name: &str, args: &Value) -> String {
    match name {
        "search_files" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let search_content = args
                .get("search_content")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let root = args
                .get("root")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("home");
            if search_content {
                format!("content match for \"{query}\" in {root}")
            } else {
                format!("name match for \"{query}\" in {root}")
            }
        }
        "read_file_preview" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            format!("read {path}")
        }
        "get_current_time" => "inspect current local time".to_string(),
        "list_processes" => "inspect running processes".to_string(),
        "web_search" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            format!("search web for {query}")
        }
        "fetch_url_preview" => {
            let url = args.get("url").and_then(Value::as_str).unwrap_or("");
            format!("fetch {url}")
        }
        "create_text_file" => {
            let filename = args.get("filename").and_then(Value::as_str).unwrap_or("");
            let destination = args
                .get("destination")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("desktop");
            format!("create {filename} in {destination}")
        }
        "open_item" => {
            let target = args.get("target").and_then(Value::as_str).unwrap_or("");
            format!("open {target}")
        }
        "get_system_info" => "inspect system info".to_string(),
        _ => truncate_for_summary(&args.to_string(), 120),
    }
}

pub fn summarize_tool_result(result: &str) -> String {
    let single_line = result.replace('\n', " ");
    truncate_for_summary(single_line.trim(), 160)
}

pub async fn execute_tool_call(
    name: &str,
    args: Value,
    safe_mode: bool,
    client: &reqwest::Client,
) -> Result<String, String> {
    match name {
        "search_files" => {
            let parsed: SearchFilesArgs = serde_json::from_value(args)
                .map_err(|e| format!("Invalid search_files arguments: {e}"))?;
            search_files(parsed)
        }
        "read_file_preview" => {
            let parsed: ReadFilePreviewArgs = serde_json::from_value(args)
                .map_err(|e| format!("Invalid read_file_preview arguments: {e}"))?;
            read_file_preview(parsed)
        }
        "get_current_time" => get_current_time(),
        "list_processes" => list_processes(),
        "web_search" => {
            let parsed: WebSearchArgs = serde_json::from_value(args)
                .map_err(|e| format!("Invalid web_search arguments: {e}"))?;
            web_search(parsed, client).await
        }
        "fetch_url_preview" => {
            let parsed: FetchUrlPreviewArgs = serde_json::from_value(args)
                .map_err(|e| format!("Invalid fetch_url_preview arguments: {e}"))?;
            fetch_url_preview(parsed, client).await
        }
        "create_text_file" => {
            let parsed: CreateTextFileArgs = serde_json::from_value(args)
                .map_err(|e| format!("Invalid create_text_file arguments: {e}"))?;
            create_text_file(parsed, safe_mode)
        }
        "open_item" => {
            let parsed: OpenItemArgs = serde_json::from_value(args)
                .map_err(|e| format!("Invalid open_item arguments: {e}"))?;
            open_item(parsed)
        }
        "get_system_info" => Ok(get_system_info()),
        _ => Err(format!("Unknown tool: {name}")),
    }
}

fn search_files(args: SearchFilesArgs) -> Result<String, String> {
    let query = args.query.trim();
    if query.is_empty() {
        return Err("search query cannot be empty".to_string());
    }

    let query_lower = query.to_lowercase();
    let search_terms = build_search_terms(&query_lower);
    let extension_query = parse_extension_query(&query_lower);
    let roots = prioritized_search_roots(args.root.as_deref())?;
    let mut results = Vec::new();
    let mut seen_hits = HashSet::new();
    let mut visited = 0usize;

    for root in &roots {
        if !root.exists() {
            continue;
        }

        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(should_descend)
        {
            if results.len() >= MAX_SEARCH_RESULTS || visited >= MAX_VISITED_ENTRIES {
                break;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            visited += 1;

            if args.search_content {
                if let Some(hit) = match_entry_path(&entry, &search_terms) {
                    if seen_hits.insert(hit.clone()) {
                        results.push(hit);
                    }
                }
                if entry.file_type().is_file() {
                    if let Some(hit) = match_file_content(&entry, &query_lower) {
                        if seen_hits.insert(hit.clone()) {
                            results.push(hit);
                        }
                    }
                }
            } else if let Some(extension) = extension_query.as_deref() {
                if entry.file_type().is_file() && file_has_extension(entry.path(), extension) {
                    let hit = entry.path().display().to_string();
                    if seen_hits.insert(hit.clone()) {
                        results.push(hit);
                    }
                }
            } else if let Some(hit) = match_entry_path(&entry, &search_terms) {
                if seen_hits.insert(hit.clone()) {
                    results.push(hit);
                }
            }

            if results.len() >= MAX_SEARCH_RESULTS || visited >= MAX_VISITED_ENTRIES {
                break;
            }
        }
    }

    if results.is_empty() {
        let searched_under = roots
            .iter()
            .take(4)
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let searched_under = if searched_under.is_empty() {
            "the configured search roots".to_string()
        } else {
            searched_under
        };
        return Ok(format!(
            "No matches found for \"{}\" under {}.",
            query,
            searched_under
        ));
    }

    let mut body = results.join("\n");
    if visited >= MAX_VISITED_ENTRIES {
        body.push_str("\n[search truncated after visiting many entries]");
    } else if results.len() >= MAX_SEARCH_RESULTS {
        body.push_str("\n[search truncated after max results]");
    }

    Ok(truncate_tool_output(body))
}

fn read_file_preview(args: ReadFilePreviewArgs) -> Result<String, String> {
    let path = resolve_read_path(args.path.trim())?;
    let metadata = fs::metadata(&path).map_err(|e| format!("Could not stat file: {e}"))?;
    if metadata.is_dir() {
        return Err("Path points to a directory, not a file.".to_string());
    }

    let bytes = fs::read(&path).map_err(|e| format!("Could not read file: {e}"))?;
    let text = decode_text_lossy(&bytes);
    let preview = truncate_by_chars(&text, MAX_FILE_PREVIEW_CHARS);
    let truncated_suffix = if text.chars().count() > MAX_FILE_PREVIEW_CHARS {
        "\n[file preview truncated]"
    } else {
        ""
    };

    Ok(format!(
        "Path: {}\nSize: {} bytes\n\n{}{}",
        path.display(),
        metadata.len(),
        preview,
        truncated_suffix
    ))
}

fn list_processes() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .output()
            .map_err(|e| format!("Failed to list processes: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                "tasklist exited with an error".to_string()
            } else {
                stderr
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut rows = stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(25)
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        if rows.is_empty() {
            rows.push("No processes returned.".to_string());
        }

        return Ok(truncate_tool_output(rows.join("\n")));
    }

    #[allow(unreachable_code)]
    Err("Process listing is only implemented for Windows in this build.".to_string())
}

fn get_current_time() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-Date -Format \"yyyy-MM-dd HH:mm:ss zzz\"",
            ])
            .output()
            .map_err(|e| format!("Could not read local time: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                "Could not read local time.".to_string()
            } else {
                format!("Could not read local time: {stderr}")
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            Err("Could not read local time.".to_string())
        } else {
            Ok(format!("Local PC time: {stdout}"))
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("Local time lookup is only implemented for Windows in this build.".to_string())
    }
}

async fn web_search(args: WebSearchArgs, client: &reqwest::Client) -> Result<String, String> {
    let query = args.query.trim();
    if query.is_empty() {
        return Err("search query cannot be empty".to_string());
    }

    let mut url =
        reqwest::Url::parse("https://html.duckduckgo.com/html/").map_err(|e| e.to_string())?;
    url.query_pairs_mut().append_pair("q", query);

    let response = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .send()
        .await
        .map_err(|e| format!("Web search failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Web search failed: HTTP {}",
            response.status().as_u16()
        ));
    }

    let html = response
        .text()
        .await
        .map_err(|e| format!("Could not read search response: {e}"))?;
    let results = parse_duckduckgo_results(&html);
    if results.is_empty() {
        return Ok(format!("No web results found for \"{query}\"."));
    }

    Ok(truncate_tool_output(results.join("\n\n")))
}

async fn fetch_url_preview(
    args: FetchUrlPreviewArgs,
    client: &reqwest::Client,
) -> Result<String, String> {
    let url = args.url.trim();
    if url.is_empty() {
        return Err("url cannot be empty".to_string());
    }

    let response = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .send()
        .await
        .map_err(|e| format!("Failed to fetch URL: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch URL: HTTP {}",
            response.status().as_u16()
        ));
    }

    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Could not read URL body: {e}"))?;

    let preview = if content_type.contains("html") || body.contains("<html") {
        html_preview(&body)
    } else {
        truncate_by_chars(body.trim(), MAX_FILE_PREVIEW_CHARS)
    };

    Ok(format!(
        "URL: {final_url}\nContent-Type: {content_type}\n\n{}",
        truncate_tool_output(preview)
    ))
}

fn create_text_file(args: CreateTextFileArgs, safe_mode: bool) -> Result<String, String> {
    let mut filename = args.filename.trim().to_string();
    if filename.is_empty() {
        return Err("filename cannot be empty".to_string());
    }
    if filename.chars().any(|ch| matches!(ch, '\\' | '/' | ':')) {
        return Err("filename must not contain path separators".to_string());
    }
    if Path::new(&filename).extension().is_none() {
        filename.push_str(".txt");
    }

    let directory = resolve_write_destination(args.destination.as_deref())?;
    if safe_mode {
        let desktop = desktop_dir().ok_or_else(|| "Could not resolve Desktop path.".to_string())?;
        if args.overwrite || directory != desktop {
            return Err(
                "Blocked by safe mode. In safe mode, only creating a new text file on the Desktop is allowed."
                    .to_string(),
            );
        }
    }
    fs::create_dir_all(&directory)
        .map_err(|e| format!("Could not prepare target directory: {e}"))?;

    let path = directory.join(filename);
    if path.exists() && !args.overwrite {
        return Err(format!(
            "File already exists: {}. Retry with overwrite=true if replacement is intended.",
            path.display()
        ));
    }

    fs::write(&path, args.content.as_bytes())
        .map_err(|e| format!("Could not write file: {e}"))?;

    let written_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(format!(
        "Created text file successfully.\nPath: {}\nBytes written: {}",
        path.display(),
        written_bytes
    ))
}

fn open_item(args: OpenItemArgs) -> Result<String, String> {
    let target = args.target.trim();
    if target.is_empty() {
        return Err("target cannot be empty".to_string());
    }

    let resolved = resolve_open_target(target);

    #[cfg(target_os = "windows")]
    {
        let user_args = args.arguments.unwrap_or_default();
        let resolved_display = resolved.to_string_lossy().to_string();
        let target_display = if resolved.exists() || looks_like_url(target) {
            resolved_display.clone()
        } else {
            target.to_string()
        };

        if looks_like_url(target) {
            spawn_windows_process(
                "cmd",
                &[
                    "/C".to_string(),
                    "start".to_string(),
                    "".to_string(),
                    target.to_string(),
                ],
            )?;
            return Ok(format!(
                "Launch request sent.\nTarget: {}\nMethod: default browser",
                target
            ));
        }

        if resolved.is_dir() {
            spawn_windows_process("explorer.exe", &[resolved_display.clone()])?;
            return Ok(format!(
                "Launch request sent.\nTarget: {}\nMethod: Explorer",
                resolved_display
            ));
        }

        if resolved.is_file() && user_args.is_empty() && prefers_notepad(&resolved) {
            spawn_windows_process("notepad.exe", &[resolved_display.clone()])?;
            return Ok(format!(
                "Launch request sent.\nTarget: {}\nMethod: Notepad",
                resolved_display
            ));
        }

        if resolved.is_file() && !user_args.is_empty() && is_executable_path(&resolved) {
            spawn_windows_process(&resolved_display, &user_args)?;
            return Ok(format!(
                "Launch request sent.\nTarget: {}\nMethod: executable path",
                resolved_display
            ));
        }

        if resolved.is_file() {
            spawn_windows_process(
                "cmd",
                &[
                    "/C".to_string(),
                    "start".to_string(),
                    "".to_string(),
                    resolved_display.clone(),
                ],
            )?;
            return Ok(format!(
                "Launch request sent.\nTarget: {}\nMethod: default app",
                resolved_display
            ));
        }

        spawn_windows_process(target, &user_args)?;
        return Ok(format!(
            "Launch request sent.\nTarget: {}\nMethod: application",
            target_display
        ));
    }

    #[allow(unreachable_code)]
    Err("Opening items is only implemented for Windows in this build.".to_string())
}

fn get_system_info() -> String {
    let username = env::var("USERNAME").unwrap_or_else(|_| "unknown".to_string());
    let computer = env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".to_string());
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let cwd = env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    format!(
        "OS: {}\nArchitecture: {}\nUser: {}\nComputer: {}\nHome: {}\nCurrent directory: {}",
        env::consts::OS,
        env::consts::ARCH,
        username,
        computer,
        home,
        cwd
    )
}

fn resolve_search_root(root: Option<&str>) -> Result<PathBuf, String> {
    let trimmed = root.unwrap_or("").trim();
    if trimmed.is_empty() {
        return home_dir().ok_or_else(|| "Could not resolve home directory.".to_string());
    }

    if let Some(path) = resolve_special_root(trimmed) {
        return Ok(path);
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        if path.exists() {
            return Ok(path);
        }
        if let Some(remapped) = remap_absolute_path(&path) {
            return Ok(remapped);
        }
        return Ok(path);
    }

    Err(
        "search root must be an absolute path or one of: home, desktop, documents, downloads"
            .to_string(),
    )
}

fn resolve_open_target(target: &str) -> PathBuf {
    if looks_like_url(target) {
        return PathBuf::from(target);
    }

    if let Ok(path) = resolve_read_path(target) {
        return path;
    }

    if let Some(path) = resolve_special_root(target) {
        return path;
    }

    let candidate = PathBuf::from(target.trim());
    if candidate.is_absolute() {
        if let Some(remapped) = remap_absolute_path(&candidate) {
            return remapped;
        }
        return candidate;
    }

    candidate
}

fn looks_like_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn prefers_notepad(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("txt")
            | Some("md")
            | Some("log")
            | Some("json")
            | Some("toml")
            | Some("yaml")
            | Some("yml")
            | Some("ini")
            | Some("cfg")
            | Some("csv")
    )
}

fn is_executable_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("exe") | Some("bat") | Some("cmd") | Some("com")
    )
}

fn spawn_windows_process(program: &str, arguments: &[String]) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(arguments);
    command
        .spawn()
        .map_err(|e| format!("Failed to launch {program}: {e}"))?;
    Ok(())
}

fn resolve_write_destination(destination: Option<&str>) -> Result<PathBuf, String> {
    let trimmed = destination.unwrap_or("desktop").trim();
    if trimmed.is_empty() {
        return desktop_dir().ok_or_else(|| "Could not resolve Desktop path.".to_string());
    }

    if let Some(path) = resolve_special_root(trimmed) {
        return Ok(path);
    }

    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(
            "destination must be one of: desktop, documents, downloads, home, or an absolute directory path"
                .to_string(),
        );
    }
    Ok(path)
}

fn resolve_read_path(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("path cannot be empty".to_string());
    }

    if let Some(path) = resolve_special_root(trimmed) {
        return Ok(path);
    }

    let candidate = PathBuf::from(trimmed);
    if candidate.is_absolute() {
        if candidate.exists() {
            return Ok(candidate);
        }
        if let Some(remapped) = remap_absolute_path(&candidate) {
            if remapped.exists() {
                return Ok(remapped);
            }
        }
    } else {
        if let Some(mapped) = map_relative_to_known_root(&candidate) {
            if mapped.exists() {
                return Ok(mapped);
            }
        }

        let file_name = candidate.file_name().unwrap_or_else(|| OsStr::new(trimmed));
        for directory in common_lookup_dirs() {
            let joined = directory.join(file_name);
            if joined.exists() {
                return Ok(joined);
            }
        }
    }

    Err(format!(
        "Could not resolve file path: {trimmed}. Use an absolute path, a desktop/documents/downloads alias, or the exact file name."
    ))
}

fn resolve_special_root(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "~" | "home" => return home_dir(),
        "desktop" => return desktop_dir(),
        "documents" | "docs" => return documents_dir(),
        "downloads" => return downloads_dir(),
        _ => {}
    }

    let candidate = PathBuf::from(trimmed);
    if !candidate.is_absolute() {
        return map_relative_to_known_root(&candidate);
    }

    remap_absolute_path(&candidate)
}

fn map_relative_to_known_root(candidate: &Path) -> Option<PathBuf> {
    let mut components = candidate.components();
    let first = components.next()?;
    let root = match first.as_os_str().to_string_lossy().to_ascii_lowercase().as_str() {
        "~" | "home" => home_dir()?,
        "desktop" => desktop_dir()?,
        "documents" | "docs" => documents_dir()?,
        "downloads" => downloads_dir()?,
        _ => return None,
    };

    let mut mapped = root;
    for component in components {
        mapped.push(component.as_os_str());
    }
    Some(mapped)
}

fn remap_absolute_path(path: &Path) -> Option<PathBuf> {
    let file_name_lower = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase());

    match file_name_lower.as_deref() {
        Some("desktop") => return desktop_dir(),
        Some("documents") | Some("docs") => return documents_dir(),
        Some("downloads") => return downloads_dir(),
        _ => {}
    }

    if let Some(parent) = path.parent() {
        let parent_name = parent
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_ascii_lowercase());

        let mapped_root = match parent_name.as_deref() {
            Some("desktop") => desktop_dir(),
            Some("documents") | Some("docs") => documents_dir(),
            Some("downloads") => downloads_dir(),
            _ => None,
        };

        if let (Some(root), Some(name)) = (mapped_root, path.file_name()) {
            return Some(root.join(name));
        }
    }

    if path_starts_with_users_root(path) {
        let parent_is_users_root = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|segment| segment.eq_ignore_ascii_case("users"));
        if parent_is_users_root {
            return home_dir();
        }
        if let Some(file_name) = path.file_name() {
            return home_dir().map(|home| home.join(file_name));
        }
        return home_dir();
    }

    None
}

fn path_starts_with_users_root(path: &Path) -> bool {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .next()
        .is_some_and(|segment| segment.eq_ignore_ascii_case("users"))
}

fn common_lookup_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = desktop_dir() {
        dirs.push(path);
    }
    if let Some(path) = documents_dir() {
        dirs.push(path);
    }
    if let Some(path) = downloads_dir() {
        dirs.push(path);
    }
    if let Some(path) = one_drive_dir() {
        dirs.push(path);
    }
    if let Some(path) = home_dir() {
        dirs.push(path);
    }
    dirs
}

fn prioritized_search_roots(root: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let trimmed = root.unwrap_or("").trim();
    if !trimmed.is_empty() {
        let resolved = resolve_search_root(Some(trimmed))?;
        if !resolved.exists() {
            return Err(format!("Search root does not exist: {}", resolved.display()));
        }
        return Ok(vec![resolved]);
    }

    let mut roots = Vec::new();
    for candidate in [
        desktop_dir(),
        documents_dir(),
        downloads_dir(),
        one_drive_dir(),
        home_dir(),
    ]
    .into_iter()
    .flatten()
    {
        if !roots.iter().any(|existing| existing == &candidate) {
            roots.push(candidate);
        }
    }

    for candidate in additional_drive_roots() {
        if !roots.iter().any(|existing| existing == &candidate) {
            roots.push(candidate);
        }
    }

    if roots.is_empty() {
        return Err("Could not resolve any search roots.".to_string());
    }

    Ok(roots)
}

fn home_dir() -> Option<PathBuf> {
    env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

fn desktop_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join("Desktop"))
}

fn documents_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join("Documents"))
}

fn downloads_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join("Downloads"))
}

fn one_drive_dir() -> Option<PathBuf> {
    env::var("OneDrive")
        .ok()
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join("OneDrive")))
}

fn additional_drive_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let known_roots = [
            desktop_dir(),
            documents_dir(),
            downloads_dir(),
            one_drive_dir(),
            home_dir(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|path| drive_root(&path))
        .collect::<HashSet<_>>();

        let mut drives = Vec::new();
        for letter in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", letter as char));
            if root.exists() && !known_roots.contains(&root) {
                drives.push(root);
            }
        }
        drives
    }

    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

fn drive_root(path: &Path) -> Option<PathBuf> {
    let display = path.display().to_string();
    let bytes = display.as_bytes();
    if bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
        Some(PathBuf::from(&display[..3]))
    } else {
        None
    }
}

fn should_descend(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return true;
    }

    let name = entry.file_name().to_string_lossy().to_lowercase();
    !matches!(
        name.as_str(),
        ".git"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "appdata"
            | "__pycache__"
            | ".venv"
            | ".cache"
            | "venv"
    )
}

fn build_search_terms(query_lower: &str) -> Vec<String> {
    let trimmed = query_lower.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut terms = vec![trimmed.to_string()];
    terms.extend(
        trimmed
            .split(|ch: char| !ch.is_alphanumeric() && !('\u{0400}'..='\u{04FF}').contains(&ch))
            .map(str::trim)
            .filter(|term| term.chars().count() >= 3)
            .map(ToString::to_string),
    );
    if trimmed.chars().count() >= 6 {
        terms.push(trimmed.chars().take(5).collect());
    }
    if let Some(stemmed) = stem_cyrillic_term(trimmed) {
        terms.push(stemmed);
    }
    terms.sort();
    terms.dedup();
    terms
}

fn parse_extension_query(query_lower: &str) -> Option<String> {
    let trimmed = query_lower.trim();
    if let Some(extension) = trimmed.strip_prefix("*.") {
        return Some(extension.to_string()).filter(|value| !value.is_empty());
    }
    if trimmed.starts_with('.') && trimmed.len() > 1 {
        return Some(trimmed.trim_start_matches('.').to_string());
    }
    None
}

fn stem_cyrillic_term(value: &str) -> Option<String> {
    let suffixes = [
        "ами", "ями", "ого", "ему", "ому", "ах", "ях", "ом", "ам", "ям", "ой", "ей", "ую",
        "юю", "ия", "ье", "ию", "ью", "иям", "иях", "а", "я", "у", "ю", "е", "ы", "и", "о",
    ];

    if !value.chars().any(|ch| ('\u{0400}'..='\u{04FF}').contains(&ch)) {
        return None;
    }

    for suffix in suffixes {
        if let Some(stem) = value.strip_suffix(suffix) {
            if stem.chars().count() >= 4 {
                return Some(stem.to_string());
            }
        }
    }

    None
}

fn match_entry_path(entry: &DirEntry, search_terms: &[String]) -> Option<String> {
    let file_name = entry.file_name().to_string_lossy().to_lowercase();
    let full_path = entry.path().display().to_string();
    let full_path_lower = full_path.to_lowercase();

    let matched = search_terms.iter().any(|term| {
        file_name.contains(term)
            || full_path_lower.contains(term)
            || path_components_match(entry.path(), term)
    });

    if !matched {
        return None;
    }

    Some(if entry.file_type().is_dir() {
        format!("{full_path} [dir]")
    } else {
        full_path
    })
}

fn file_has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn parse_duckduckgo_results(html: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut remaining = html;

    while let Some(anchor_start) = remaining.find("result__a") {
        remaining = &remaining[anchor_start..];
        let Some(href_start) = remaining.find("href=\"") else {
            break;
        };
        let href_part = &remaining[href_start + 6..];
        let Some(href_end) = href_part.find('"') else {
            break;
        };
        let url = normalize_search_result_url(&html_decode(&href_part[..href_end]));

        let Some(title_tag_end) = href_part[href_end..].find('>') else {
            break;
        };
        let title_part = &href_part[href_end + title_tag_end + 1..];
        let Some(title_end) = title_part.find("</a>") else {
            break;
        };
        let title = html_decode(&strip_html_tags(&title_part[..title_end]));

        let mut snippet = String::new();
        if let Some(snippet_start) = title_part[title_end..].find("result__snippet") {
            let snippet_part = &title_part[title_end + snippet_start..];
            if let Some(snippet_tag_end) = snippet_part.find('>') {
                let snippet_body = &snippet_part[snippet_tag_end + 1..];
                if let Some(snippet_end) = snippet_body.find("</a>") {
                    snippet = html_decode(&strip_html_tags(&snippet_body[..snippet_end]));
                } else if let Some(snippet_end) = snippet_body.find("</div>") {
                    snippet = html_decode(&strip_html_tags(&snippet_body[..snippet_end]));
                }
            }
        }

        if !title.trim().is_empty() && !url.trim().is_empty() {
            results.push(format!(
                "{}\n{}\n{}",
                title.trim(),
                url.trim(),
                truncate_for_summary(snippet.trim(), 220)
            ));
        }

        if results.len() >= 5 {
            break;
        }
        remaining = &title_part[title_end..];
    }

    results
}

fn html_preview(html: &str) -> String {
    let title = html
        .split("<title>")
        .nth(1)
        .and_then(|part| part.split("</title>").next())
        .map(|value| html_decode(value.trim()))
        .unwrap_or_default();
    let text = collapse_whitespace(&html_decode(&strip_html_tags(html)));
    let text = truncate_by_chars(text.trim(), MAX_FILE_PREVIEW_CHARS);

    if title.is_empty() {
        text
    } else {
        format!("Title: {title}\n\n{text}")
    }
}

fn normalize_search_result_url(raw: &str) -> String {
    let decoded = html_decode(raw.trim());
    if let Ok(url) = reqwest::Url::parse(&decoded) {
        if url.domain().is_some_and(|domain| domain.contains("duckduckgo.com")) {
            if let Some(target) = url
                .query_pairs()
                .find(|(key, _)| key == "uddg")
                .map(|(_, value)| value.into_owned())
            {
                return target;
            }
        }
        return decoded;
    }

    if decoded.starts_with("/l/?") || decoded.starts_with("https://duckduckgo.com/l/?") {
        let absolute = if decoded.starts_with("/l/?") {
            format!("https://duckduckgo.com{decoded}")
        } else {
            decoded.clone()
        };
        if let Ok(url) = reqwest::Url::parse(&absolute) {
            if let Some(target) = url
                .query_pairs()
                .find(|(key, _)| key == "uddg")
                .map(|(_, value)| value.into_owned())
            {
                return target;
            }
        }
    }

    decoded
}

fn strip_html_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn html_decode(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn path_components_match(path: &Path, term: &str) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .to_lowercase()
            .contains(term)
    })
}

fn match_file_content(entry: &DirEntry, query_lower: &str) -> Option<String> {
    let metadata = entry.metadata().ok()?;
    if metadata.len() > MAX_FILE_SCAN_BYTES {
        return None;
    }

    if !looks_like_text_file(entry.path()) {
        return None;
    }

    let bytes = fs::read(entry.path()).ok()?;
    let text = decode_text_lossy(&bytes);
    for (line_idx, line) in text.lines().enumerate() {
        if line.to_lowercase().contains(query_lower) {
            let snippet = truncate_for_summary(line.trim(), 140);
            return Some(format!(
                "{}:{}: {}",
                entry.path().display(),
                line_idx + 1,
                snippet
            ));
        }
    }

    None
}

fn looks_like_text_file(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());

    matches!(
        extension.as_deref(),
        Some("txt")
            | Some("md")
            | Some("json")
            | Some("toml")
            | Some("yaml")
            | Some("yml")
            | Some("ini")
            | Some("cfg")
            | Some("log")
            | Some("csv")
            | Some("ts")
            | Some("tsx")
            | Some("js")
            | Some("jsx")
            | Some("rs")
            | Some("py")
            | Some("html")
            | Some("css")
            | Some("java")
            | Some("kt")
            | Some("cs")
            | Some("cpp")
            | Some("c")
            | Some("h")
            | Some("hpp")
            | Some("go")
            | Some("php")
            | Some("rb")
            | Some("swift")
            | Some("sql")
            | Some("xml")
            | Some("bat")
            | Some("ps1")
    )
}

fn decode_text_lossy(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    String::from_utf8_lossy(bytes).into_owned()
}

fn truncate_tool_output(output: String) -> String {
    truncate_by_chars(&output, MAX_TOOL_OUTPUT_CHARS)
}

fn truncate_for_summary(input: &str, max_chars: usize) -> String {
    let truncated = truncate_by_chars(input, max_chars);
    if input.chars().count() > max_chars {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn truncate_by_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: StdMutex<()> = StdMutex::new(());

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn unique_temp_home() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("thukiwin-agent-test-{suffix}"))
    }

    #[test]
    fn decode_text_lossy_handles_utf16le_bom() {
        let bytes = vec![0xFF, 0xFE, b'H', 0, b'i', 0];
        assert_eq!(decode_text_lossy(&bytes), "Hi");
    }

    #[test]
    fn summarize_tool_args_describes_search_mode() {
        let summary = summarize_tool_args(
            "search_files",
            &json!({
                "query": "todo",
                "search_content": true,
                "root": "C:\\Users\\me"
            }),
        );

        assert!(summary.contains("content match"));
        assert!(summary.contains("todo"));
    }

    #[test]
    fn summarize_tool_result_truncates_long_output() {
        let result = summarize_tool_result(&"a".repeat(400));
        assert!(result.len() <= 163);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn create_text_file_is_blocked_in_safe_mode() {
        let result = create_text_file(
            CreateTextFileArgs {
                filename: "note.txt".to_string(),
                content: "hello".to_string(),
                destination: Some("C:\\temp".to_string()),
                overwrite: false,
            },
            true,
        );

        assert!(result.unwrap_err().contains("Blocked by safe mode"));
    }

    #[test]
    fn tool_system_prompt_mentions_safe_mode() {
        assert!(tool_system_prompt(true).contains("Safe mode is ON"));
        assert!(tool_system_prompt(false).contains("Safe mode is OFF"));
    }

    #[test]
    fn resolve_special_root_maps_desktop_alias() {
        let _guard = env_guard();
        let home = home_dir().expect("home dir");
        let desktop = resolve_special_root("desktop").expect("desktop dir");
        assert_eq!(desktop, home.join("Desktop"));
    }

    #[test]
    fn resolve_search_root_repairs_wrong_user_profile_path() {
        let _guard = env_guard();
        let expected = home_dir().expect("home dir");
        let repaired = resolve_search_root(Some("C:\\Users\\WrongUser"))
            .expect("repaired home path");
        assert_eq!(repaired, expected);
    }

    #[test]
    fn resolve_read_path_finds_bare_filename_on_desktop() {
        let _guard = env_guard();
        let temp_home = unique_temp_home();
        let desktop = temp_home.join("Desktop");
        fs::create_dir_all(&desktop).unwrap();
        let file = desktop.join("cpp_joke.txt");
        fs::write(&file, "hello").unwrap();

        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("HOME", &temp_home);

        let resolved = resolve_read_path("cpp_joke.txt").expect("resolved file");
        assert_eq!(resolved, file);

        if let Some(value) = old_userprofile {
            std::env::set_var("USERPROFILE", value);
        } else {
            std::env::remove_var("USERPROFILE");
        }
        if let Some(value) = old_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }

        fs::remove_dir_all(&temp_home).unwrap();
    }

    #[test]
    fn resolve_open_target_keeps_application_name_when_no_file_matches() {
        assert_eq!(resolve_open_target("notepad"), PathBuf::from("notepad"));
    }

    #[test]
    fn resolve_open_target_finds_desktop_file_by_name() {
        let _guard = env_guard();
        let temp_home = unique_temp_home();
        let desktop = temp_home.join("Desktop");
        fs::create_dir_all(&desktop).unwrap();
        let file = desktop.join("joke.txt");
        fs::write(&file, "hello").unwrap();

        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("HOME", &temp_home);

        let resolved = resolve_open_target("joke.txt");
        assert_eq!(resolved, file);

        if let Some(value) = old_userprofile {
            std::env::set_var("USERPROFILE", value);
        } else {
            std::env::remove_var("USERPROFILE");
        }
        if let Some(value) = old_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }

        fs::remove_dir_all(&temp_home).unwrap();
    }

    #[test]
    fn prefers_notepad_for_txt_files() {
        assert!(prefers_notepad(Path::new("joke.txt")));
        assert!(!prefers_notepad(Path::new("photo.png")));
    }

    #[test]
    fn match_entry_path_returns_directory_hits() {
        let _guard = env_guard();
        let temp_home = unique_temp_home();
        let diploma_dir = temp_home.join("Desktop").join("Диплом");
        fs::create_dir_all(&diploma_dir).unwrap();

        let entry = WalkDir::new(&temp_home)
            .into_iter()
            .filter_map(Result::ok)
            .find(|entry| entry.path() == diploma_dir)
            .expect("directory entry");

        let hit = match_entry_path(&entry, &build_search_terms("диплом")).expect("directory hit");
        assert!(hit.contains("[dir]"));

        fs::remove_dir_all(&temp_home).unwrap();
    }
}
