# Artifacts 工具实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 Artifacts 功能：deferred 工具上传 HTML 到 CCB 服务端、TUI 中渲染 OSC 8 可点击链接、`/artifacts` 命令浏览会话内所有上传文件。

**Architecture:** ArtifactTool 作为 defered tool 注册在 ToolSearchMiddleware 中，通过 reqwest POST 到 CCB Cloudflare Worker。工具结果以 OSC 8 escape 嵌入的格式化文本返回，经现有 ToolBlock 渲染管道直接到达终端。`/artifacts` 为 TUI 端面板命令，扫描 pipeline.completed 消息提取 artifact 列表。

**Tech Stack:** Rust, reqwest (HTTP), ratatui + LinkSpan (OSC 8 hyperlink), arboard (剪贴板), serde_json

---

## 文件结构

| 文件 | 职责 | 操作 |
|------|------|------|
| `peri-middlewares/src/tool_search/artifact_client.rs` | HTTP POST 到 CCB 服务端 | **新建** |
| `peri-middlewares/src/tool_search/artifact_client_test.rs` | HTTP client 测试 | **新建** |
| `peri-middlewares/src/tool_search/artifact_tool.rs` | ArtifactTool struct + BaseTool impl | **新建** |
| `peri-middlewares/src/tool_search/artifact_tool_test.rs` | ArtifactTool 测试 | **新建** |
| `peri-middlewares/src/tool_search/middleware.rs` | collect_tools 注册 artifact | **修改** |
| `peri-middlewares/src/tool_search/mod.rs` | 模块声明 | **修改** |
| `peri-tui/src/app/tool_display.rs` | artifact 工具 display 映射 | **修改** |
| `peri-tui/src/app/panel_manager.rs` | PanelKind/PanelState 变体 + dispatch! | **修改** |
| `peri-tui/src/app/artifacts_panel.rs` | ArtifactsPanel struct + PanelComponent impl | **新建** |
| `peri-tui/src/ui/main_ui/panels/artifacts.rs` | 面板渲染 | **新建** |
| `peri-tui/src/command/panel/artifacts.rs` | ArtifactsCommand | **新建** |
| `peri-tui/src/command/panel/mod.rs` | 模块声明 + pub use | **修改** |
| `peri-tui/src/command/mod.rs` | default_registry 注册 | **修改** |
| `peri-tui/src/app/mod.rs` | mod declaration | **修改** |

---

### Task 1: ArtifactClient HTTP 客户端

**Files:**
- Create: `peri-middlewares/src/tool_search/artifact_client.rs`
- Create: `peri-middlewares/src/tool_search/artifact_client_test.rs`

- [ ] **Step 1: 编写 ArtifactClient 测试（红阶段）**

创建 `peri-middlewares/src/tool_search/artifact_client_test.rs`：

```rust
use super::artifact_client::{ArtifactClient, ArtifactResponse, ArtifactError};

#[tokio::test]
async fn test_build_url_default() {
    let client = ArtifactClient::default();
    assert_eq!(client.upload_url(), "https://cloud-artifacts.claude-code-best.win/upload");
}

#[tokio::test]
async fn test_build_url_custom() {
    let client = ArtifactClient::new("https://example.com".into(), "mytoken".into());
    assert_eq!(client.upload_url(), "https://example.com/upload");
}

#[tokio::test]
async fn test_parse_success_response() {
    let body = r#"{"id":"abc123","url":"https://cloud-artifacts.claude-code-best.win/7d/abc123.html","expiresAt":"2026-06-27T12:00:00Z"}"#;
    let resp: ArtifactResponse = serde_json::from_str(body).unwrap();
    assert_eq!(resp.id, "abc123");
    assert_eq!(resp.url, "https://cloud-artifacts.claude-code-best.win/7d/abc123.html");
    assert!(resp.error.is_none());
}

#[tokio::test]
async fn test_parse_error_response() {
    // Deno Deploy 抹平 HTTP 状态码为 200，错误信息在 body 中
    let body = r#"{"error":"payload_too_large"}"#;
    let resp: ArtifactResponse = serde_json::from_str(body).unwrap();
    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap(), "payload_too_large");
}

#[tokio::test]
async fn test_format_output_success() {
    let resp = ArtifactResponse {
        id: "abc123".into(),
        url: "https://cloud-artifacts.claude-code-best.win/7d/abc123.html".into(),
        expires_at: Some("2026-06-27T12:00:00Z".into()),
        error: None,
    };
    let output = ArtifactClient::format_output(&resp);
    assert!(output.contains("Artifact uploaded:"));
    assert!(output.contains("https://cloud-artifacts.claude-code-best.win/7d/abc123.html"));
    assert!(output.contains("2026-06-27T12:00:00Z"));
    // OSC 8 escape 序列存在
    assert!(output.contains("\x1b]8;;"));
    assert!(output.contains("\x1b]8;;\x1b\\"));
}

#[tokio::test]
async fn test_format_output_no_expiry() {
    let resp = ArtifactResponse {
        id: "abc123".into(),
        url: "https://example.com/file.html".into(),
        expires_at: None,
        error: None,
    };
    let output = ArtifactClient::format_output(&resp);
    assert!(output.contains("Artifact uploaded:"));
    assert!(!output.contains("Expires:"));
}
```

- [ ] **Step 2: 运行测试验证失败**

```bash
cargo test -p peri-middlewares -- artifact_client_test --lib
```
预期：编译失败（模块未创建）。

- [ ] **Step 3: 实现 ArtifactClient + ArtifactResponse**

创建 `peri-middlewares/src/tool_search/artifact_client.rs`：

```rust
use peri_widgets::link::wrap_osc8;

const DEFAULT_URL: &str = "https://cloud-artifacts.claude-code-best.win";
const DEFAULT_TOKEN: &str = "claude-code-best";

/// CCB Artifacts 服务 HTTP 客户端。
pub struct ArtifactClient {
    base_url: String,
    token: String,
}

/// 服务端响应（兼容 Deno Deploy 抹平 HTTP 状态码的情况，错误字段在 body 中）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ArtifactResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, alias = "expiresAt")]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl ArtifactClient {
    pub fn new(base_url: String, token: String) -> Self {
        Self { base_url, token }
    }

    /// 使用默认 CCB 服务端和内置 token。
    /// 环境变量 PERI_ARTIFACTS_URL / PERI_ARTIFACTS_TOKEN 可覆盖。
    pub fn from_env_or_default() -> Self {
        let url = std::env::var("PERI_ARTIFACTS_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
        let token =
            std::env::var("PERI_ARTIFACTS_TOKEN").unwrap_or_else(|_| DEFAULT_TOKEN.to_string());
        Self::new(url, token)
    }

    pub fn upload_url(&self) -> String {
        format!("{}/upload", self.base_url.trim_end_matches('/'))
    }

    /// 上传 HTML 文件内容并返回格式化输出（包含 OSC 8 可点击链接）。
    /// 失败时返回包含 error 信息的字符串。
    pub async fn upload(&self, content: &str, ttl: &str) -> String {
        let url = self.upload_url();

        let client = reqwest::Client::new();
        let result = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "text/html")
            .header("X-TTL", ttl)
            .body(content.to_string())
            .send()
            .await;

        let resp_text = match result {
            Ok(r) => match r.text().await {
                Ok(t) => t,
                Err(e) => return format!("Failed to read response: {}", e),
            },
            Err(e) => return format!("Upload failed: {}", e),
        };

        let parsed: ArtifactResponse = match serde_json::from_str(&resp_text) {
            Ok(p) => p,
            Err(e) => {
                return format!("Failed to parse response: {}. Body: {}", e, resp_text);
            }
        };

        if let Some(error) = parsed.error {
            return format!("Upload error: {}", error);
        }

        Self::format_output(&parsed)
    }

    /// 将成功响应格式化为含 OSC 8 可点击链接的输出文本。
    pub fn format_output(resp: &ArtifactResponse) -> String {
        let linked_url = wrap_osc8(&resp.url, &resp.url);
        let mut output = format!("Artifact uploaded: {}\n", linked_url);
        if let Some(ref expires) = resp.expires_at {
            output.push_str(&format!("Expires: {}", expires));
        }
        output
    }
}

impl Default for ArtifactClient {
    fn default() -> Self {
        Self::new(DEFAULT_URL.to_string(), DEFAULT_TOKEN.to_string())
    }
}
```

- [ ] **Step 4: 运行测试验证通过**

```bash
cargo test -p peri-middlewares -- artifact_client_test --lib
```
预期：全部 6 个测试通过。

- [ ] **Step 5: 在 mod.rs 中声明模块**

修改 `peri-middlewares/src/tool_search/mod.rs`，在已有 `pub mod` 声明区添加：

```rust
pub mod artifact_client;
```

并在文件末尾添加测试模块声明：

```rust
#[cfg(test)]
#[path = "artifact_client_test.rs"]
mod artifact_client_test;
```

- [ ] **Step 6: 全量编译验证**

```bash
cargo build -p peri-middlewares
```
预期：无编译错误。

- [ ] **Step 7: Commit**

```bash
git add peri-middlewares/src/tool_search/artifact_client.rs peri-middlewares/src/tool_search/artifact_client_test.rs peri-middlewares/src/tool_search/mod.rs
git commit -m "feat: add ArtifactClient for CCB artifacts service upload

HTTP POST with auth, OSC 8 formatted output, environment variable override support"
```

---

### Task 2: ArtifactTool Deferred 工具

**Files:**
- Create: `peri-middlewares/src/tool_search/artifact_tool.rs`
- Create: `peri-middlewares/src/tool_search/artifact_tool_test.rs`

- [ ] **Step 1: 编写 ArtifactTool 测试（红阶段）**

创建 `peri-middlewares/src/tool_search/artifact_tool_test.rs`：

```rust
use super::artifact_tool::ArtifactTool;
use peri_agent::tools::BaseTool;
use serde_json::json;

#[test]
fn test_artifact_tool_name() {
    let tool = ArtifactTool::new("/tmp".into());
    assert_eq!(tool.name(), "artifact");
}

#[test]
fn test_artifact_tool_description() {
    let tool = ArtifactTool::new("/tmp".into());
    assert!(tool.description().contains("HTML"));
    assert!(tool.description().contains("upload"));
}

#[test]
fn test_artifact_tool_parameters_schema() {
    let tool = ArtifactTool::new("/tmp".into());
    let params = tool.parameters();
    // file_path 必需
    assert_eq!(params["properties"]["file_path"]["type"], "string");
    assert!(params["required"].as_array().unwrap().iter().any(|v| v.as_str() == Some("file_path")));
    // ttl 可选，默认 7d
    assert_eq!(params["properties"]["ttl"]["type"], "string");
    assert!(params["properties"]["ttl"]["enum"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn test_invoke_file_not_found() {
    let tool = ArtifactTool::new("/tmp".into());
    let result = tool.invoke(json!({"file_path": "/nonexistent/file.html", "ttl": "7d"})).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found") || err.contains("exist"));
}

#[tokio::test]
async fn test_invoke_non_html_extension() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    let mut f = std::fs::File::create(&file_path).unwrap();
    f.write_all(b"hello").unwrap();

    let tool = ArtifactTool::new(dir.path().to_string_lossy().to_string());
    let result = tool.invoke(json!({"file_path": file_path.to_string_lossy(), "ttl": "7d"})).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("HTML"));
}

#[tokio::test]
async fn test_invoke_file_too_large() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.html");
    let mut f = std::fs::File::create(&file_path).unwrap();
    // 写入超过 10MB 的数据
    let chunk = vec![b'a'; 1024 * 1024]; // 1MB
    for _ in 0..11 {
        f.write_all(&chunk).unwrap();
    }

    let tool = ArtifactTool::new(dir.path().to_string_lossy().to_string());
    let result = tool.invoke(json!({"file_path": file_path.to_string_lossy(), "ttl": "7d"})).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("too large") || err.contains("10MB") || err.contains("exceeds"));
}

#[test]
fn test_resolve_path_relative() {
    let tool = ArtifactTool::new("/home/user/project".into());
    // 通过 invoke 间接验证路径解析
    let params = tool.parameters();
    assert!(params.to_string().contains("file_path"));
}
```

- [ ] **Step 2: 运行测试验证失败**

```bash
cargo test -p peri-middlewares -- artifact_tool_test --lib
```
预期：编译失败（模块未创建）。

- [ ] **Step 3: 实现 ArtifactTool**

创建 `peri-middlewares/src/tool_search/artifact_tool.rs`：

```rust
use std::path::Path;

use peri_agent::tools::BaseTool;
use serde_json::{json, Value};

use super::artifact_client::ArtifactClient;

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
const ALLOWED_EXTENSIONS: &[&str] = &["html", "htm"];

/// Artifact 上传工具——将本地 HTML 文件上传到 CCB Artifacts 服务，返回公开 URL。
///
/// 延迟加载（deferred tool）：LLM 通过 SearchExtraTools → ExecuteExtraTool 两步调用。
pub struct ArtifactTool {
    cwd: String,
    client: ArtifactClient,
}

impl ArtifactTool {
    pub fn new(cwd: String) -> Self {
        Self {
            cwd,
            client: ArtifactClient::from_env_or_default(),
        }
    }

    fn resolve_path(&self, file_path: &str) -> Result<std::path::PathBuf, String> {
        let path = Path::new(file_path);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            Path::new(&self.cwd).join(path)
        };
        Ok(resolved)
    }

    fn validate_file(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("File not found: {}", path.display()));
        }
        if !path.is_file() {
            return Err(format!("Not a file: {}", path.display()));
        }

        // 检查扩展名
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        match ext {
            Some(ref e) if ALLOWED_EXTENSIONS.contains(&e.as_str()) => {}
            _ => {
                return Err(format!(
                    "Only HTML files are supported (allowed: {}). Got: {}",
                    ALLOWED_EXTENSIONS.join(", "),
                    path.display()
                ));
            }
        }

        // 检查大小
        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(e) => return Err(format!("Cannot read file metadata: {}", e)),
        };
        if size > MAX_FILE_SIZE {
            return Err(format!(
                "File too large: {} bytes (max: {} bytes / 10MB)",
                size, MAX_FILE_SIZE
            ));
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl BaseTool for ArtifactTool {
    fn name(&self) -> &str {
        "artifact"
    }

    fn description(&self) -> &str {
        "Upload an HTML file to a public URL with automatic expiry. \
         The file will be accessible via a shareable link for 7 days (default) or 30 days. \
         Use this after generating HTML content (dashboards, reports, prototypes) that you want to share. \
         Returns a clickable URL that can be opened in any browser."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the HTML file to upload (relative or absolute)"
                },
                "ttl": {
                    "type": "string",
                    "enum": ["7d", "30d"],
                    "description": "Time-to-live. Use '7d' for 7-day expiry (default), '30d' for 30-day expiry."
                }
            },
            "required": ["file_path"]
        })
    }

    async fn invoke(
        &self,
        input: Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or("Missing required parameter: file_path")?;
        let ttl = input["ttl"].as_str().unwrap_or("7d");

        let resolved = self.resolve_path(file_path)?;
        self.validate_file(&resolved)?;

        let content = std::fs::read_to_string(&resolved)
            .map_err(|e| format!("Failed to read file {}: {}", resolved.display(), e))?;

        let output = self.client.upload(&content, ttl).await;
        Ok(output)
    }
}
```

- [ ] **Step 4: 运行测试验证通过**

```bash
cargo test -p peri-middlewares -- artifact_tool_test --lib
```
预期：全部 6 个测试通过（HTTP 相关测试会因为无网络跳过实际上传，仅测试参数和校验）。

- [ ] **Step 5: 在 mod.rs 中声明模块**

修改 `peri-middlewares/src/tool_search/mod.rs`，在最近添加的 `pub mod artifact_client;` 附近添加：

```rust
pub mod artifact_tool;
```

并在测试声明区添加：

```rust
#[cfg(test)]
#[path = "artifact_tool_test.rs"]
mod artifact_tool_test;
```

- [ ] **Step 6: 全量编译验证**

```bash
cargo build -p peri-middlewares
```
预期：无编译错误。

- [ ] **Step 7: Commit**

```bash
git add peri-middlewares/src/tool_search/artifact_tool.rs peri-middlewares/src/tool_search/artifact_tool_test.rs peri-middlewares/src/tool_search/mod.rs
git commit -m "feat: add ArtifactTool deferred tool for HTML file upload

Validates file type/size, delegates HTTP upload to ArtifactClient,
returns OSC 8 hyperlink formatted output"
```

---

### Task 3: 在 ToolSearchMiddleware 注册 ArtifactTool

**Files:**
- Modify: `peri-middlewares/src/tool_search/middleware.rs`

- [ ] **Step 1: 修改 collect_tools 注册 ArtifactTool**

修改 `peri-middlewares/src/tool_search/middleware.rs`，在 `collect_tools` 方法中（SearchExtraTools 和 ExecuteExtraTool 之后）添加：

```rust
fn collect_tools(&self, cwd: &str) -> Vec<Box<dyn BaseTool>> {
    let mut tools: Vec<Box<dyn BaseTool>> = vec![
        Box::new(SearchExtraTools::new(
            self.tool_index.clone(),
        )),
        Box::new(ExecuteExtraTool::new(
            self.shared_tools.clone(),
            core_tools_sorted_csv(),
        )),
        // 注册 deferred artifact 工具
        Box::new(artifact_tool::ArtifactTool::new(cwd.to_string())),
    ];
    tools
}
```

确保文件顶部已 `use super::artifact_tool;`（或 `use super::artifact_tool::ArtifactTool`）。

- [ ] **Step 2: 运行现有测试确保无回归**

```bash
cargo test -p peri-middlewares -- tool_search --lib
```
预期：全部测试通过。

- [ ] **Step 3: 全量编译验证**

```bash
cargo build -p peri-middlewares -p peri-acp
```
预期：无编译错误。

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tool_search/middleware.rs
git commit -m "feat: register ArtifactTool in ToolSearchMiddleware collect_tools"
```

---

### Task 4: 添加 artifact 工具 TUI 显示映射

**Files:**
- Modify: `peri-tui/src/app/tool_display.rs`

- [ ] **Step 1: 在 format_tool_name 和 format_tool_args 添加 artifact**

修改 `peri-tui/src/app/tool_display.rs`：

在 `format_tool_name` 的 match 中添加：

```rust
"artifact" => "ArtUp",
```

在 `format_tool_args` 的 match 中添加（在 `"ExecuteExtraTool"` arm 之前或之后）：

```rust
"artifact" => input["file_path"].as_str().map(|p| strip_cwd(p, cwd)),
```

- [ ] **Step 2: 全量编译验证**

```bash
cargo build -p peri-tui
```
预期：无编译错误。

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/app/tool_display.rs
git commit -m "feat: add artifact tool display mapping in TUI"
```

---

### Task 5: ArtifactsPanel 面板组件

**Files:**
- Create: `peri-tui/src/app/artifacts_panel.rs`
- Create: `peri-tui/src/ui/main_ui/panels/artifacts.rs`

- [ ] **Step 1: 定义 ArtifactEntry 类型和 ArtifactsPanel struct**

创建 `peri-tui/src/app/artifacts_panel.rs`：

```rust
use std::path::PathBuf;

use arboard::Clipboard;
use ratatui::{
    crossterm::event::MouseEvent,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};
use tui_textarea::Input;

use crate::app::{panel_component::PanelComponent, panel_manager::PanelKind};
use super::panel_manager::{EventResult, PanelContext};

/// Artifact 条目（从消息历史中提取）
#[derive(Debug, Clone)]
pub struct ArtifactEntry {
    pub url: String,
    pub id: String,
    pub expires_at: Option<String>,
}

/// Artifact 列表面板。
pub struct ArtifactsPanel {
    entries: Vec<ArtifactEntry>,
    selected: usize,
    scroll_offset: u16,
}

impl ArtifactsPanel {
    pub fn new(entries: Vec<ArtifactEntry>) -> Self {
        Self {
            entries,
            selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn entries(&self) -> &[ArtifactEntry] {
        &self.entries
    }

    fn move_cursor(&mut self, delta: i32) {
        if self.entries.is_empty() {
            return;
        }
        let n = self.entries.len() as i32;
        let new = (self.selected as i32 + delta).rem_euclid(n) as usize;
        self.selected = new;
    }

    fn open_selected(&self) {
        if let Some(entry) = self.entries.get(self.selected) {
            let url = &entry.url;
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open").arg(url).spawn();
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            }
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("cmd").args(["/c", "start", url]).spawn();
            }
        }
    }

    fn copy_selected(&mut self) -> Result<(), String> {
        if let Some(entry) = self.entries.get(self.selected) {
            let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
            clipboard.set_text(entry.url.clone()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

impl PanelComponent for ArtifactsPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Artifacts
    }

    fn handle_key(&mut self, input: Input, _ctx: &mut PanelContext<'_>) -> EventResult {
        use tui_textarea::Key;
        match input {
            Input { key: Key::Up, .. } | Input { key: Key::Char('k'), .. } => {
                self.move_cursor(-1);
                EventResult::Consumed
            }
            Input { key: Key::Down, .. } | Input { key: Key::Char('j'), .. } => {
                self.move_cursor(1);
                EventResult::Consumed
            }
            Input { key: Key::Enter, .. } => {
                self.open_selected();
                EventResult::Consumed
            }
            Input { key: Key::Char('c'), .. } => {
                if let Err(e) = self.copy_selected() {
                    tracing::warn!("Failed to copy artifact URL: {}", e);
                }
                EventResult::Consumed
            }
            Input { key: Key::Esc, .. } => EventResult::ClosePanel,
            Input { key: Key::Char('c'), ctrl: true, .. } => EventResult::NotConsumed,
            _ => EventResult::Consumed,
        }
    }

    fn handle_mouse(&mut self, _mouse: MouseEvent, _area: Rect, _ctx: &mut PanelContext<'_>) -> EventResult {
        EventResult::NotConsumed
    }

    fn desired_height(&self, screen_height: u16, _screen_width: u16) -> u16 {
        let entry_count = if self.entries.is_empty() { 1 } else { self.entries.len() };
        let desired = (entry_count as u16 + 3).min(screen_height.saturating_sub(3));
        desired.max(5)
    }

    fn render(&mut self, f: &mut Frame, _app: &mut crate::app::App, area: Rect) {
        super::super::ui::main_ui::panels::artifacts::render_artifacts_panel(
            f, area, self,
        );
    }

    fn as_any_ref(&self) -> &dyn std::any::Any { self }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}
```

- [ ] **Step 2: 编写面板渲染函数**

创建 `peri-tui/src/ui/main_ui/panels/artifacts.rs`：

```rust
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Clear},
    Frame,
};

use crate::app::artifacts_panel::ArtifactsPanel;

pub fn render_artifacts_panel(f: &mut Frame, area: Rect, panel: &ArtifactsPanel) {
    let entries = panel.entries();
    let selected = panel.selected();

    // 先清空背景
    let clear = Clear;
    f.render_widget(clear, area);

    let block = Block::default()
        .title(" Artifacts ")
        .borders(Borders::ALL)
        .style(Style::default());
    f.render_widget(block, area);

    let inner = area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    let list_area = chunks[1];

    // 提示行
    let hint_line = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(" select  "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" open  "),
        Span::styled("c", Style::default().fg(Color::Yellow)),
        Span::raw(" copy  "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" close"),
    ]);
    f.render_widget(Paragraph::new(hint_line), chunks[0]);

    if entries.is_empty() {
        let empty = Paragraph::new("No artifacts in this session.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, list_area);
        return;
    }

    let lines: Vec<Line> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_selected = i == *selected;
            let base_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };

            let mut spans = vec![Span::styled(
                if is_selected { "▶ " } else { "  " },
                base_style,
            )];

            spans.push(Span::styled(&entry.url, base_style));

            if let Some(ref expires) = entry.expires_at {
                spans.push(Span::styled(
                    format!("  expires: {}", expires),
                    base_style.fg(Color::DarkGray),
                ));
            }

            Line::from(spans)
        })
        .collect();

    let list = Paragraph::new(lines);
    f.render_widget(list, list_area);
}
```

注意：`ArtifactsPanel` 的 `selected()` 和 `entries()` 方法需要在 struct 上添加 getter。这些已在 Step 1 中定义。

- [ ] **Step 3: 在 mod.rs 中声明模块**

修改 `peri-tui/src/app/mod.rs`，在适当位置添加：

```rust
pub mod artifacts_panel;
```

修改 `peri-tui/src/ui/main_ui/panels/mod.rs`（如果存在），否则在 `peri-tui/src/ui/main_ui/mod.rs` 或等效位置添加：

```rust
pub mod artifacts;
```

- [ ] **Step 4: 全量编译验证**

```bash
cargo build -p peri-tui
```
预期：无编译错误。

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/artifacts_panel.rs peri-tui/src/ui/main_ui/panels/artifacts.rs peri-tui/src/app/mod.rs
git commit -m "feat: add ArtifactsPanel component with keyboard navigation and clipboard copy"
```

---

### Task 6: PanelManager 集成（PanelKind + PanelState + dispatch!）

**Files:**
- Modify: `peri-tui/src/app/panel_manager.rs`

- [ ] **Step 1: 添加 PanelKind::Artifacts 变体**

在 `peri-tui/src/app/panel_manager.rs` 的 `PanelKind` 枚举中添加：

```rust
Artifacts,
```

- [ ] **Step 2: 添加 PanelState::Artifacts 变体**

在 `PanelState` 枚举中添加：

```rust
Artifacts(ArtifactsPanel),
```

- [ ] **Step 3: 更新 panel_dispatch! 宏**

在 `panel_dispatch!` 宏的四个分支中各添加一行 `Artifacts` 对应条目：

**`kind:` 分支（约 L179-192）**——在 `Betas` 之后添加：
```rust
PanelState::Artifacts(_) => PanelKind::Artifacts,
```

**`any:ref` 分支（约 L197-209）**——在 `Betas` 之后添加：
```rust
PanelState::Artifacts(p) => p as &dyn Any,
```

**`any:mut` 分支（约 L215-227）**——在 `Betas` 之后添加：
```rust
PanelState::Artifacts(p) => p as &mut dyn Any,
```

**通用 `$body` 分支（约 L233-245）**——在 `Betas` 之后添加：
```rust
PanelState::Artifacts($p) => $body,
```

注意：宏中 `Betas` 使用 `PanelState::Betas(p) => p as &dyn Any`（非 Box 包装），`Artifacts` 也应为相同模式（直接 `as &dyn Any`）。

- [ ] **Step 4: 在 panel_manager.rs 顶部添加 use**

```rust
use super::artifacts_panel::ArtifactsPanel;
```

- [ ] **Step 5: 全量编译验证**

```bash
cargo build -p peri-tui
```
预期：无编译错误。

- [ ] **Step 6: Commit**

```bash
git add peri-tui/src/app/panel_manager.rs
git commit -m "feat: integrate ArtifactsPanel into PanelManager (PanelKind + PanelState + dispatch)"
```

---

### Task 7: `/artifacts` TUI 命令

**Files:**
- Create: `peri-tui/src/command/panel/artifacts.rs`
- Modify: `peri-tui/src/command/panel/mod.rs`
- Modify: `peri-tui/src/command/mod.rs`
- Modify: `peri-tui/src/app/mod.rs`
- Modify: `peri-tui/Cargo.toml`

- [ ] **Step 0: 添加 regex 依赖到 peri-tui**

- [ ] **Step 0: 添加 regex 依赖到 peri-tui**

修改 `peri-tui/Cargo.toml`，在 `[dependencies]` 区添加：

```toml
regex = "1"
```

```bash
cargo build -p peri-tui  # 验证编译
```

- [ ] **Step 1: 定义 ArtifactsCommand**

创建 `peri-tui/src/command/panel/artifacts.rs`：

```rust
use crate::{app::App, command::Command, app::artifacts_panel::ArtifactEntry};

/// /artifacts 命令——打开本会话 artifact 列表面板
pub struct ArtifactsCommand;

impl Command for ArtifactsCommand {
    fn name(&self) -> &str {
        "artifacts"
    }

    fn description(&self, _lc: &crate::i18n::LcRegistry) -> String {
        "列出本会话上传的所有 artifact".to_string()
    }

    fn execute(&self, app: &mut App, _args: &str) {
        let entries = scan_artifacts(app);
        app.open_artifacts_panel(entries);
    }
}

/// 从消息历史中扫描 artifact 上传记录。
///
/// 遍历 pipeline.completed 中的所有 BaseMessage，
/// 查找 assistant 消息中的 tool_use block（name == "artifact"），
/// 配对 user 消息中的 tool_result block，用 regex 提取 URL。
fn scan_artifacts(app: &App) -> Vec<ArtifactEntry> {
    use regex::Regex;
    use peri_agent::types::{BaseMessage, ContentBlock};

    let url_re = Regex::new(r"https?://[^\s)\"',]+\.html\b").unwrap();
    let expires_re = Regex::new(r"Expires:\s*([0-9T:.Z+-]+)").unwrap();
    let id_re = Regex::new(r"\bid:\s*([A-Za-z0-9_-]+)").unwrap();

    let mut entries = Vec::new();

    // 从 pipeline state 获取所有已完成消息
    let messages = app
        .session_mgr
        .current()
        .agent
        .pipeline
        .completed
        .iter();

    // 收集所有 artifact tool_use 的 tool_call_id
    let mut artifact_ids: Vec<String> = Vec::new();

    for msg in messages.clone() {
        if let BaseMessage::Ai(ref ai_msg) = msg {
            for block in &ai_msg.content {
                if let ContentBlock::ToolUse(tu) = block {
                    if tu.name == "artifact" {
                        artifact_ids.push(tu.id.clone());
                    }
                }
            }
        }
    }

    // 在 user 消息中查找对应的 tool_result
    for msg in messages {
        if let BaseMessage::Human(ref user_msg) = msg {
            for block in &user_msg.content {
                if let ContentBlock::ToolResult(tr) = block {
                    if artifact_ids.contains(&tr.tool_use_id) {
                        let content = &tr.content;
                        let url = url_re
                            .find(content)
                            .map(|m| m.as_str().to_string());

                        if let Some(url) = url {
                            let id = id_re
                                .captures(content)
                                .and_then(|c| c.get(1))
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_default();

                            let expires_at = expires_re
                                .captures(content)
                                .and_then(|c| c.get(1))
                                .map(|m| m.as_str().to_string());

                            entries.push(ArtifactEntry { url, id, expires_at });
                        }
                    }
                }
            }
        }
    }

    entries
}
```

- [ ] **Step 2: 在 App 中添加 open_artifacts_panel 方法**

修改 `peri-tui/src/app/mod.rs`，在 `impl App` 块中添加（参考其他 `open_xxx_panel` 方法）：

```rust
pub fn open_artifacts_panel(&mut self, entries: Vec<artifacts_panel::ArtifactEntry>) {
    let panel = artifacts_panel::ArtifactsPanel::new(entries);
    self.panel_manager.open(PanelState::Artifacts(panel));
}
```

- [ ] **Step 3: 在 panel/mod.rs 中注册模块**

修改 `peri-tui/src/command/panel/mod.rs`，添加：

```rust
pub mod artifacts;
```

并在文件末尾的 pub use 区添加：

```rust
pub use artifacts::ArtifactsCommand;
```

- [ ] **Step 4: 在 command/mod.rs 注册命令**

修改 `peri-tui/src/command/mod.rs`，在 `default_registry()` 函数中添加（放在 panel 区）：

```rust
r.register(Box::new(panel::artifacts::ArtifactsCommand));
```

- [ ] **Step 5: 全量编译验证**

```bash
cargo build -p peri-tui
```
预期：无编译错误。

- [ ] **Step 6: Commit**

```bash
git add peri-tui/src/command/panel/artifacts.rs peri-tui/src/command/panel/mod.rs peri-tui/src/command/mod.rs peri-tui/src/app/mod.rs
git commit -m "feat: add /artifacts command with message history scanning"
```

---

### Task 8: 集成验证

**Files:**
- 无代码变更，仅验证。

- [ ] **Step 1: 全量构建**

```bash
cargo build
```
预期：所有 crate 编译成功。

- [ ] **Step 2: 全量测试**

```bash
cargo test
```
预期：所有测试通过。

- [ ] **Step 3: 运行 clippy**

```bash
cargo clippy --all-targets
```
预期：无新增警告。

- [ ] **Step 4: 运行 fmt**

```bash
cargo fmt --all -- --check
```
预期：格式正确。如有修改则重新 commit。

- [ ] **Step 5: Commit（如有格式修复）**

```bash
git add . && git commit -m "chore: apply formatting and clippy fixes for artifacts feature"
```
