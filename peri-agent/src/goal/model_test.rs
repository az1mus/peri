use super::*;
use chrono::Utc;

#[test]
fn test_thread_goal_new_生成有效_goal_id() {
    let goal = ThreadGoal::new("完成 PR review".to_string(), None);
    assert_eq!(goal.objective, "完成 PR review");
    assert_eq!(goal.status, GoalStatus::Active);
    assert_eq!(goal.token_budget, None);
    assert!(!goal.goal_id.is_empty());
    assert!(goal.created_at <= Utc::now());
}

#[test]
fn test_thread_goal_with_budget() {
    let goal = ThreadGoal::new("重构模块".to_string(), Some(200_000));
    assert_eq!(goal.token_budget, Some(200_000));
}

#[test]
fn test_thread_goal_serde_roundtrip() {
    let goal = ThreadGoal::new("测试序列化".to_string(), Some(100_000));
    let json = serde_json::to_string(&goal).unwrap();
    let deserialized: ThreadGoal = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.objective, goal.objective);
    assert_eq!(deserialized.token_budget, goal.token_budget);
}

#[test]
fn test_goal_status_转换合法() {
    use GoalStatus::*;
    // Active 可以 → Paused / Complete / Blocked / BudgetLimited
    assert!(Active.can_transition_to(&Paused));
    assert!(Active.can_transition_to(&Complete));
    assert!(Active.can_transition_to(&Blocked));
    assert!(Active.can_transition_to(&BudgetLimited));
    // Paused 可以 → Active
    assert!(Paused.can_transition_to(&Active));
    // Complete 是终态，不能转换
    assert!(!Complete.can_transition_to(&Active));
}
