mod executor;
mod loader;
pub mod template;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use anyhow::Context;
use sqlx::SqlitePool;
use tokio::sync::Semaphore;

use crate::db::{NodeRun, WorkflowRun};
use crate::runner::template::TemplateContext;
use crate::schema::{NodeDef, Workflow};

pub use loader::load_workflow;
pub use loader::load_workflow_from_content;

const MAX_CONCURRENT_NODES: usize = 16;

/// Run a workflow to completion asynchronously.
/// This spawns the actual execution so the caller (HTTP handler) can return immediately.
pub async fn run_workflow(
    pool: Arc<SqlitePool>,
    run_id: String,
    workflow: Workflow,
    inputs: HashMap<String, String>,
) {
    tokio::spawn(async move {
        if let Err(e) = execute_dag(pool.clone(), &run_id, &workflow, &inputs).await {
            tracing::error!(run_id = %run_id, error = %e, "workflow execution failed");
            let _ =
                WorkflowRun::update_status(&pool, &run_id, "failed", Some(&e.to_string())).await;
        }
    });
}

/// Execute the full DAG: schedule → run nodes → finalize.
async fn execute_dag(
    pool: Arc<SqlitePool>,
    run_id: &str,
    wf: &Workflow,
    inputs: &HashMap<String, String>,
) -> anyhow::Result<()> {
    let nodes = &wf.nodes;

    WorkflowRun::set_started(&pool, run_id).await?;

    let levels = topological_sort(nodes)?;

    let node_index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (node_id(n), i))
        .collect();

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_NODES));

    let mut completed: HashSet<usize> = HashSet::new();
    let mut failed: HashSet<usize> = HashSet::new();

    // In-memory output tracking: node_id -> outputs
    let mut completed_outputs: HashMap<String, HashMap<String, String>> = HashMap::new();

    for level in &levels {
        let mut tasks = Vec::new();

        for &idx in level {
            let node = &nodes[idx];
            let deps_ready = node_depends(node).iter().all(|dep| {
                node_index
                    .get(dep.as_str())
                    .is_some_and(|&di| completed.contains(&di) || failed.contains(&di))
            });

            if !deps_ready {
                tracing::warn!(node_id = %node_id(node), "dependencies not met, skipping");
                continue;
            }

            // Build template context for this node
            let ctx = build_template_context(
                node,
                inputs,
                &wf.reference_inputs,
                &wf.env,
                &completed_outputs,
            );

            let pool = pool.clone();
            let semaphore = semaphore.clone();
            let run_id = run_id.to_string();
            let node = node.clone();

            let task = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                executor::execute_node(&pool, &run_id, &node, &ctx).await
            });

            tasks.push((idx, task));
        }

        for (idx, task) in tasks {
            match task.await {
                Ok(Ok(outputs)) => {
                    let nid = node_id(&nodes[idx]).to_string();
                    if !outputs.is_empty() {
                        completed_outputs.insert(nid, outputs);
                    }
                    completed.insert(idx);
                }
                Ok(Err(e)) => {
                    tracing::error!(node_idx = idx, error = %e, "node failed");
                    failed.insert(idx);
                }
                Err(e) => {
                    tracing::error!(node_idx = idx, error = %e, "node task panicked");
                    failed.insert(idx);
                }
            }
        }

        if !failed.is_empty() {
            for &fi in &failed {
                let node = &nodes[fi];
                if !node_continue_on_error(node) {
                    WorkflowRun::update_status(
                        &pool,
                        run_id,
                        "failed",
                        Some(&format!("node '{}' failed", node_id(node))),
                    )
                    .await?;
                    // Mark remaining pending nodes as skipped
                    let _ = NodeRun::mark_run_pending_as_skipped(&pool, run_id).await;
                    return Err(anyhow::anyhow!("node '{}' failed", node_id(node)));
                }
            }
        }
    }

    WorkflowRun::update_status(&pool, run_id, "success", None).await?;
    tracing::info!(run_id = %run_id, "workflow completed successfully");
    Ok(())
}

/// Build the template context for a node.
fn build_template_context(
    node: &NodeDef,
    root_inputs: &HashMap<String, String>,
    reference_inputs: &HashMap<String, HashMap<String, String>>,
    global_env: &HashMap<String, String>,
    completed_outputs: &HashMap<String, HashMap<String, String>>,
) -> TemplateContext {
    let nid = node_id(node);

    // Determine effective inputs: if node ID has a prefix (e.g. "do-build/checkout"),
    // look up reference_inputs for that prefix.
    let effective_inputs = if let Some(slash_pos) = nid.find('/') {
        let prefix = &nid[..slash_pos];
        reference_inputs
            .get(prefix)
            .cloned()
            .unwrap_or_else(|| root_inputs.clone())
    } else {
        root_inputs.clone()
    };

    // Build env: start with global, then interpolate and merge node env
    let node_env = get_node_env(node);
    let mut env = global_env.clone();
    // Interpolate node env with global-only context first (avoid circularity)
    let pre_ctx = TemplateContext {
        inputs: effective_inputs.clone(),
        needs_outputs: completed_outputs.clone(),
        env: global_env.clone(),
    };
    let resolved_node_env = crate::runner::template::interpolate_map(&node_env, &pre_ctx);
    env.extend(resolved_node_env);

    TemplateContext {
        inputs: effective_inputs,
        needs_outputs: completed_outputs.clone(),
        env,
    }
}

fn get_node_env(node: &NodeDef) -> HashMap<String, String> {
    match node {
        NodeDef::Shell(n) => n.env.clone(),
        NodeDef::Agent(n) => n.env.clone(),
        NodeDef::Reference(_) => HashMap::new(),
    }
}

/// Topological sort returning levels of node indices that can run in parallel.
fn topological_sort(nodes: &[NodeDef]) -> anyhow::Result<Vec<Vec<usize>>> {
    let n = nodes.len();
    let id_to_idx: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (node_id(n), i))
        .collect();

    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    let mut in_degree = vec![0u32; n];

    for (i, node) in nodes.iter().enumerate() {
        for dep in node_depends(node) {
            let j = id_to_idx.get(dep.as_str()).with_context(|| {
                format!("node '{}' depends on unknown node '{}'", node_id(node), dep)
            })?;
            adj[*j].push(i);
            in_degree[i] += 1;
        }
    }

    let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut levels: Vec<Vec<usize>> = Vec::new();

    while !queue.is_empty() {
        let current_level: Vec<usize> = queue.drain(..).collect();
        levels.push(current_level.clone());

        let mut next_queue = VecDeque::new();
        for &node in &current_level {
            for &neighbor in &adj[node] {
                in_degree[neighbor] -= 1;
                if in_degree[neighbor] == 0 {
                    next_queue.push_back(neighbor);
                }
            }
        }
        queue = next_queue;
    }

    if levels.iter().map(|l| l.len()).sum::<usize>() != n {
        anyhow::bail!("workflow contains a cycle");
    }

    Ok(levels)
}

// ─── Node Helpers ─────────────────────────────────────────────────

pub fn node_id(node: &NodeDef) -> &str {
    match node {
        NodeDef::Shell(n) => &n.id,
        NodeDef::Agent(n) => &n.id,
        NodeDef::Reference(n) => &n.id,
    }
}

pub fn node_depends(node: &NodeDef) -> &[String] {
    match node {
        NodeDef::Shell(n) => &n.depends,
        NodeDef::Agent(n) => &n.depends,
        NodeDef::Reference(n) => &n.depends,
    }
}

pub fn node_type_name(node: &NodeDef) -> &str {
    match node {
        NodeDef::Shell(_) => "shell",
        NodeDef::Agent(_) => "agent",
        NodeDef::Reference(_) => "reference",
    }
}

fn node_continue_on_error(node: &NodeDef) -> bool {
    match node {
        NodeDef::Shell(n) => n.continue_on_error,
        NodeDef::Agent(n) => n.continue_on_error,
        NodeDef::Reference(n) => n.continue_on_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ExecConfig, ShellNode};

    fn make_shell_node(id: &str, depends: Vec<String>, continue_on_error: bool) -> NodeDef {
        NodeDef::Shell(ShellNode {
            id: id.to_string(),
            run: crate::schema::ScriptSource::Inline("echo".to_string()),
            depends,
            outputs: Default::default(),
            env: Default::default(),
            continue_on_error,
            exec: ExecConfig {
                timeout: None,
                retry: None,
                shell: None,
            },
        })
    }

    #[test]
    fn test_topological_sort_simple() {
        let nodes = vec![
            make_shell_node("a", vec![], false),
            make_shell_node("b", vec!["a".to_string()], false),
            make_shell_node("c", vec!["b".to_string()], false),
        ];
        let levels = topological_sort(&nodes).unwrap();
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], vec![0]); // a
        assert_eq!(levels[1], vec![1]); // b
        assert_eq!(levels[2], vec![2]); // c
    }

    #[test]
    fn test_topological_sort_parallel() {
        let nodes = vec![
            make_shell_node("a", vec![], false),
            make_shell_node("b", vec![], false),
            make_shell_node("c", vec!["a".to_string(), "b".to_string()], false),
        ];
        let levels = topological_sort(&nodes).unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].len(), 2); // a, b in parallel
        assert_eq!(levels[1], vec![2]); // c
    }

    #[test]
    fn test_topological_sort_cycle() {
        let nodes = vec![
            make_shell_node("a", vec!["b".to_string()], false),
            make_shell_node("b", vec!["a".to_string()], false),
        ];
        assert!(topological_sort(&nodes).is_err());
    }

    #[test]
    fn test_topological_sort_unknown_dep() {
        let nodes = vec![make_shell_node("a", vec!["nonexistent".to_string()], false)];
        assert!(topological_sort(&nodes).is_err());
    }

    #[test]
    fn test_node_continue_on_error() {
        let node = make_shell_node("x", vec![], true);
        assert!(node_continue_on_error(&node));
        let node2 = make_shell_node("y", vec![], false);
        assert!(!node_continue_on_error(&node2));
    }
}
