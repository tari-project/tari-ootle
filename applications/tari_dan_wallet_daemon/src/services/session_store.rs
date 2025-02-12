// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Debug, Error)]
pub enum SessionStoreError {
    #[error("Session not found: {session_id}")]
    SessionNotFound { session_id: String },
}

/// A trait the every session data must implement.
pub trait SessionData: Clone {
    fn created_at(&self) -> Instant;
}

/// A thread-safe store that acts like a classical Session storage for web, but uses unique IDs for a session
/// instead of using cookies, so it is suitable for stateless RPC calls.
#[derive(Debug, Clone)]
pub struct SessionStore<T: SessionData> {
    sessions: Arc<RwLock<HashMap<String, T>>>,
    session_ttl: Duration,
}

impl<T: SessionData> SessionStore<T> {
    pub fn new(session_ttl: Duration) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_ttl,
        }
    }

    async fn remove_expired_sessions(&self) {
        let mut lock = self.sessions.write().await;
        let expired_sessions: Vec<String> = lock
            .iter_mut()
            .filter(|(_, session)| session.created_at().elapsed() > self.session_ttl)
            .map(|(key, _)| key.clone())
            .collect();
        expired_sessions.iter().for_each(|key| {
            lock.remove(key);
        })
    }

    /// Returns session by its ID.
    async fn session(&self, session_id: &str) -> Result<T, SessionStoreError> {
        let lock = self.sessions.read().await;
        lock.get(session_id).cloned().ok_or(SessionStoreError::SessionNotFound {
            session_id: String::from(session_id),
        })
    }

    /// Get session by ID.
    pub async fn get(&self, session_id: &str) -> Result<T, SessionStoreError> {
        self.remove_expired_sessions().await;
        self.session(session_id).await
    }

    /// Gets a new session ID and makes sure that its unique.
    async fn new_session_id(&self) -> String {
        let mut session_id = Uuid::new_v4().to_string();
        while let Ok(_) = self.session(&session_id).await {
            session_id = Uuid::new_v4().to_string();
        }
        session_id
    }

    /// Add new session
    pub async fn add(&self, data: T) -> Result<String, SessionStoreError> {
        self.remove_expired_sessions().await;
        let session_id = self.new_session_id().await;
        self.sessions.write().await.insert(session_id.clone(), data);
        Ok(session_id)
    }

    /// Removes session, if it does not exists, this call have no effect.
    pub async fn remove(&self, session_id: &str) -> Option<T> {
        if let Ok(session) = self.session(session_id).await {
            let result = session.clone();
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_id);
            return Some(result);
        }
        None
    }
}
