use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use rust_create_agent::messages::BaseMessage;
use rust_create_agent::thread::{ThreadId, ThreadMeta, ThreadStore};
use rust_agent_middlewares::prelude::SharedPermissionMode;
use tokio_util::sync::CancellationToken;

use crate::app::agent::LlmProvider;
use crate::config::ZenConfig;

pub struct AcpSession {
    pub session_id: String,
    pub thread_id: ThreadId,
    pub cwd: String,
    pub cancel_token: CancellationToken,
    pub state_messages: Vec<BaseMessage>,
    pub created_at: chrono::DateTime<Utc>,
}

struct SessionManagerInner {
    sessions: DashMap<String, AcpSession>,
    thread_store: Arc<dyn ThreadStore>,
    provider: LlmProvider,
    zen_config: Arc<ZenConfig>,
    permission_mode: Arc<SharedPermissionMode>,
    next_session_id: std::sync::atomic::AtomicU64,
}

#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<SessionManagerInner>,
}

impl SessionManager {
    pub fn new(
        thread_store: Arc<dyn ThreadStore>,
        provider: LlmProvider,
        zen_config: Arc<ZenConfig>,
        permission_mode: Arc<SharedPermissionMode>,
    ) -> Self {
        Self {
            inner: Arc::new(SessionManagerInner {
                sessions: DashMap::new(),
                thread_store,
                provider,
                zen_config,
                permission_mode,
                next_session_id: std::sync::atomic::AtomicU64::new(1),
            }),
        }
    }

    /// 使用指定 session_id 创建会话（用于 session/load 和 session/resume）
    pub async fn new_session_with_id(
        &self,
        session_id: &str,
        cwd: &str,
    ) -> anyhow::Result<()> {
        if self.inner.sessions.contains_key(session_id) {
            return Ok(());
        }

        let thread_id = ThreadId::from(session_id.to_string());
        let session = AcpSession {
            session_id: session_id.to_string(),
            thread_id,
            cwd: cwd.to_string(),
            cancel_token: CancellationToken::new(),
            state_messages: Vec::new(),
            created_at: Utc::now(),
        };

        self.inner.sessions.insert(session_id.to_string(), session);
        Ok(())
    }

    pub async fn new_session(&self, cwd: &str) -> anyhow::Result<(String, ThreadId)> {
        let meta = ThreadMeta::new(cwd);
        let thread_id = self.inner.thread_store.create_thread(meta).await?;

        let session_id = self
            .inner
            .next_session_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_string();

        let session = AcpSession {
            session_id: session_id.clone(),
            thread_id: thread_id.clone(),
            cwd: cwd.to_string(),
            cancel_token: CancellationToken::new(),
            state_messages: Vec::new(),
            created_at: Utc::now(),
        };

        self.inner.sessions.insert(session_id.clone(), session);
        Ok((session_id, thread_id))
    }

    pub async fn close_session(&self, session_id: &str) -> anyhow::Result<()> {
        if let Some((_, session)) = self.inner.sessions.remove(session_id) {
            session.cancel_token.cancel();
        }
        Ok(())
    }

    pub async fn list_sessions(&self) -> anyhow::Result<Vec<ThreadMeta>> {
        self.inner.thread_store.list_threads().await.map_err(Into::into)
    }

    pub fn get_session(&self, session_id: &str) -> Option<dashmap::mapref::one::Ref<'_, String, AcpSession>> {
        self.inner.sessions.get(session_id)
    }

    pub fn cancel_session(&self, session_id: &str) {
        if let Some(session) = self.inner.sessions.get(session_id) {
            session.cancel_token.cancel();
        }
    }

    pub fn provider(&self) -> &LlmProvider {
        &self.inner.provider
    }

    pub fn zen_config(&self) -> &Arc<ZenConfig> {
        &self.inner.zen_config
    }

    pub fn permission_mode(&self) -> &Arc<SharedPermissionMode> {
        &self.inner.permission_mode
    }

    pub fn thread_store(&self) -> &Arc<dyn ThreadStore> {
        &self.inner.thread_store
    }

    pub async fn load_thread_messages(
        &self,
        thread_id: &ThreadId,
    ) -> anyhow::Result<Vec<BaseMessage>> {
        self.inner
            .thread_store
            .load_messages(thread_id)
            .await
            .map_err(Into::into)
    }
}
