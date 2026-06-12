use super::*;
use peri_agent::goal::InMemoryGoalStore;
use std::sync::Arc;

fn make_state() -> GoalState {
    GoalState::new(
        Arc::new(InMemoryGoalStore::new()),
        "test-thread".to_string(),
    )
}

#[tokio::test]
async fn test_set_goal_写入_store_并触发_objective_updated() {
    let state = make_state();
    state
        .set_goal("完成模块重构".to_string(), Some(200_000))
        .await
        .unwrap();

    let snap = state.snapshot();
    assert_eq!(snap.objective.as_deref(), Some("完成模块重构"));
    assert_eq!(snap.token_budget, Some(200_000));
    assert_eq!(snap.status, Some(GoalStatus::Active));
    assert!(snap.objective_just_updated);
}

#[tokio::test]
async fn test_clear_清空_goal() {
    let state = make_state();
    state.set_goal("临时目标".to_string(), None).await.unwrap();
    state.clear().await.unwrap();

    let snap = state.snapshot();
    assert!(snap.objective.is_none());
    assert!(!snap.objective_just_updated);
}

#[tokio::test]
async fn test_set_goal_覆盖旧_goal_生成新_goal_id() {
    let state = make_state();
    state.set_goal("目标 A".to_string(), None).await.unwrap();
    let id_a = state.snapshot().goal_id.clone().unwrap();

    state.set_goal("目标 B".to_string(), None).await.unwrap();
    let id_b = state.snapshot().goal_id.clone().unwrap();

    assert_ne!(id_a, id_b);
    assert_eq!(state.snapshot().objective.as_deref(), Some("目标 B"));
}

#[tokio::test]
async fn test_store_写入失败_内存镜像仍可读() {
    use async_trait::async_trait;
    use peri_agent::goal::{GoalStore, GoalStoreError, ThreadGoal};

    struct FailingStore;
    #[async_trait]
    impl GoalStore for FailingStore {
        async fn save(&self, _: &str, _: ThreadGoal) -> Result<(), GoalStoreError> {
            Err(GoalStoreError::Io("simulated".to_string()))
        }
        async fn load(&self, _: &str) -> Result<Option<ThreadGoal>, GoalStoreError> {
            Err(GoalStoreError::Io("simulated".to_string()))
        }
        async fn delete(&self, _: &str) -> Result<(), GoalStoreError> {
            Err(GoalStoreError::Io("simulated".to_string()))
        }
    }

    let state = GoalState::new(Arc::new(FailingStore), "test-thread".to_string());
    // set_goal 即使 store 失败也不 panic（内存镜像更新成功）
    let result = state.set_goal("fallback".to_string(), None).await;
    // store 失败返回 Err，但内存镜像已更新
    assert!(result.is_err());
    assert_eq!(state.snapshot().objective.as_deref(), Some("fallback"));
}
