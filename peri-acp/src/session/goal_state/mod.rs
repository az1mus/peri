//! GoalState — goal 子系统的并发状态机。
//!
//! 基于 `Arc<RwLock<GoalStateInner>>` + `parking_lot::RwLock`（短锁无 await）。
//! store 写入失败时退化为纯内存模式（snapshot 读仍可用），不阻塞 agent。
//!
//! 并发模型：read-and-reset + epoch（本 Task 先实现基础读写，account_progress 的
//! read-and-reset 在 Task 6 实现）。

use std::sync::Arc;

use parking_lot::RwLock;
use peri_agent::goal::{GoalStatus, GoalStore, ThreadGoal};

/// Goal 快照（只读视图，供 middleware / TUI 读取）
#[derive(Debug, Clone, Default)]
pub struct GoalSnapshot {
    pub goal_id: Option<String>,
    pub objective: Option<String>,
    pub status: Option<GoalStatus>,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    /// set_goal / edit 后置 true，middleware 注入后清零
    pub objective_just_updated: bool,
}

impl GoalSnapshot {
    /// 是否有活跃的 goal
    pub fn has_active_goal(&self) -> bool {
        self.status == Some(GoalStatus::Active)
    }
}

/// 内部可变状态（受 RwLock 保护）
struct GoalStateInner {
    goal: Option<ThreadGoal>,
    /// set_goal / clear_goal 后置 true，GoalMiddleware 注入后清零
    objective_just_updated: bool,
    store: Arc<dyn GoalStore>,
    thread_id: String,
}

/// 并发安全的状态句柄
#[derive(Clone)]
pub struct GoalState {
    inner: Arc<RwLock<GoalStateInner>>,
}

impl GoalState {
    pub fn new(store: Arc<dyn GoalStore>, thread_id: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(GoalStateInner {
                goal: None,
                objective_just_updated: false,
                store,
                thread_id,
            })),
        }
    }

    /// set_goal：UPSERT（新 goal_id），触发 objective_updated。
    /// store 写入失败不回滚内存镜像（内存优于 store 原则）。
    pub async fn set_goal(
        &self,
        objective: String,
        token_budget: Option<u64>,
    ) -> Result<(), peri_agent::goal::GoalStoreError> {
        let new_goal = ThreadGoal::new(objective, token_budget);
        let (thread_id, store) = {
            let mut guard = self.inner.write();
            guard.goal = Some(new_goal.clone());
            guard.objective_just_updated = true;
            (guard.thread_id.clone(), guard.store.clone())
        };

        // 短锁：lock 在块作用域结束即释放，store.save 是长操作但不持锁
        if let Err(e) = store.save(&thread_id, new_goal).await {
            tracing::warn!(error = %e, "GoalState: store save 失败，退化为纯内存模式");
            return Err(e);
        }
        Ok(())
    }

    /// clear：清空 goal
    pub async fn clear(&self) -> Result<(), peri_agent::goal::GoalStoreError> {
        let (thread_id, store) = {
            let mut guard = self.inner.write();
            guard.goal = None;
            guard.objective_just_updated = false;
            (guard.thread_id.clone(), guard.store.clone())
        };

        if let Err(e) = store.delete(&thread_id).await {
            tracing::warn!(error = %e, "GoalState: store delete 失败");
            return Err(e);
        }
        Ok(())
    }

    /// 只读快照（短锁，立即释放）
    pub fn snapshot(&self) -> GoalSnapshot {
        let guard = self.inner.read();
        match &guard.goal {
            Some(g) => GoalSnapshot {
                goal_id: Some(g.goal_id.clone()),
                objective: Some(g.objective.clone()),
                status: Some(g.status),
                token_budget: g.token_budget,
                tokens_used: g.accounting.tokens_used,
                time_used_seconds: g.accounting.time_used_seconds,
                objective_just_updated: guard.objective_just_updated,
            },
            None => GoalSnapshot {
                objective_just_updated: guard.objective_just_updated,
                ..Default::default()
            },
        }
    }

    /// 消费 objective_just_updated 标志（middleware 注入后调用）
    pub fn consume_objective_updated(&self) -> bool {
        let mut guard = self.inner.write();
        let was_set = guard.objective_just_updated;
        guard.objective_just_updated = false;
        was_set
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
