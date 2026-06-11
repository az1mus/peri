use serde_json::json;

use super::*;

struct MockTool {
    name_str: String,
    desc_str: String,
    params: serde_json::Value,
}

impl MockTool {
    fn new(name: &str, desc: &str) -> Self {
        Self {
            name_str: name.to_string(),
            desc_str: desc.to_string(),
            params: json!({"type": "object", "properties": {}}),
        }
    }
}

#[async_trait::async_trait]
impl BaseTool for MockTool {
    fn name(&self) -> &str {
        &self.name_str
    }
    fn description(&self) -> &str {
        &self.desc_str
    }
    fn parameters(&self) -> serde_json::Value {
        self.params.clone()
    }
    async fn invoke(
        &self,
        _input: serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("mock".to_string())
    }
}

fn make_mock_tools() -> Vec<Arc<dyn BaseTool>> {
    vec![
        Arc::new(MockTool::new(
            "CronRegister",
            "Register a cron scheduled task",
        )),
        Arc::new(MockTool::new("CronList", "List all cron tasks")),
        Arc::new(MockTool::new("CronRemove", "Remove a cron task by ID")),
        Arc::new(MockTool::new(
            "mcp__slack__send_message",
            "Send a message to Slack channel",
        )),
        Arc::new(MockTool::new(
            "mcp__github__create_issue",
            "Create a GitHub issue",
        )),
    ]
}

#[test]
fn test_build_index() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);
    assert_eq!(index.list_names().len(), 5);
}

#[test]
fn test_keyword_search() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);

    let results = index.search("cron create", 3);
    assert!(!results.is_empty());
    // CronRegister should rank high
    assert!(results[0].name.contains("Cron"));
}

#[test]
fn test_tfidf_search() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);

    let results = index.search("schedule task", 3);
    assert!(!results.is_empty());
}

#[test]
fn test_hybrid_search() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);

    let results = index.search("+slack message", 5);
    // Required word "slack" should filter to only slack tools
    assert!(results
        .iter()
        .all(|r| r.name.to_lowercase().contains("slack")));
}

#[test]
fn test_get_tool() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);

    assert!(index.get_tool("CronRegister").is_some());
    assert!(index.get_tool("NonExistent").is_none());
}

#[test]
fn test_format_deferred_list() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);

    let list = index.format_deferred_list();
    assert!(list.contains("CronRegister"));
    // MCP 工具不出现在 Deferred Tools 段（避免 system prompt 不稳定导致缓存失效）
    assert!(!list.contains("mcp__slack__send_message"));
    assert!(!list.contains("mcp__github__create_issue"));
}

#[test]
fn test_total_count() {
    let index = ToolSearchIndex::new();
    assert_eq!(index.total_count(), 0);

    let tools = make_mock_tools();
    index.build(tools);
    assert_eq!(index.total_count(), 5);
}

#[test]
fn test_select_exact_match() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);

    let results = index.search("select:CronRegister,CronList", 10);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, "CronRegister");
    assert_eq!(results[1].name, "CronList");
}

#[test]
fn test_select_partial_miss() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);

    let results = index.search("select:CronRegister,NonExistent", 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "CronRegister");
}

#[test]
fn test_select_empty_result() {
    let index = ToolSearchIndex::new();
    let tools = make_mock_tools();
    index.build(tools);

    let results = index.search("select:NonExistent", 10);
    assert!(results.is_empty());
}

/// 同名工具注册覆盖语义：后注册的实例覆盖先注册的。
/// build() 通过 HashMap::insert 注册工具，name 重复时静默覆盖。
#[test]
fn test_duplicate_name_overwrites() {
    let index = ToolSearchIndex::new();
    let tools: Vec<Arc<dyn BaseTool>> = vec![
        Arc::new(MockTool::new("CronRegister", "Version A - original")),
        Arc::new(MockTool::new("CronRegister", "Version B - overwritten")),
    ];
    index.build(tools);

    // 验证只有 1 个工具注册（非 2 个）
    assert_eq!(index.total_count(), 1, "同名工具应覆盖，total_count=1");

    // 验证 get_tool 返回后注册的实例（Version B）
    let tool = index
        .get_tool("CronRegister")
        .expect("get_tool 应能查找到 CronRegister");
    assert_eq!(
        tool.description(),
        "Version B - overwritten",
        "后注册的工具应覆盖前一个，description 应为 Version B"
    );
}

/// 覆盖后 search 仍然正常工作，不会 panic 或返回错误结果。
#[test]
fn test_duplicate_name_search_still_works() {
    let index = ToolSearchIndex::new();
    let tools: Vec<Arc<dyn BaseTool>> = vec![
        Arc::new(MockTool::new("CronRegister", "Register cron tasks v1")),
        Arc::new(MockTool::new("CronRegister", "Register cron tasks v2")),
        Arc::new(MockTool::new("CronList", "List all cron tasks")),
    ];
    index.build(tools);

    // search 不应 panic
    let results = index.search("cron", 5);
    assert_eq!(
        results.len(),
        2,
        "search 应返回 2 个结果（CronRegister + CronList），实际: {}",
        results.len()
    );

    // CronRegister 的描述应为覆盖后的版本
    let cron_reg = results
        .iter()
        .find(|r| r.name == "CronRegister")
        .expect("应能找到 CronRegister");
    assert!(
        cron_reg.description.contains("v2"),
        "search 结果应反映覆盖后的描述，实际: {}",
        cron_reg.description
    );
}
