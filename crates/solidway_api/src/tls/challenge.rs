//! HTTP-01 挑战 token 的进程内共享表：ACME 客户端写入，`http_port` 的 router 读取。

use std::collections::HashMap;
use std::sync::RwLock;

/// 挑战 token 表（`token → key authorization`）。
#[derive(Debug, Default)]
pub struct ChallengeStore {
    tokens: RwLock<HashMap<String, String>>,
}

impl ChallengeStore {
    pub fn insert(&self, token: &str, key_authorization: &str) {
        let mut tokens = self.tokens.write().expect("challenge lock");
        tokens.insert(token.to_string(), key_authorization.to_string());
    }

    pub fn remove(&self, token: &str) {
        let mut tokens = self.tokens.write().expect("challenge lock");
        tokens.remove(token);
    }

    pub fn get(&self, token: &str) -> Option<String> {
        let tokens = self.tokens.read().expect("challenge lock");
        tokens.get(token).cloned()
    }

    pub fn clear(&self) {
        let mut tokens = self.tokens.write().expect("challenge lock");
        tokens.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_removes_tokens() {
        let store = ChallengeStore::default();
        store.insert("tok", "tok.thumb");
        assert_eq!(store.get("tok").as_deref(), Some("tok.thumb"));
        store.remove("tok");
        assert!(store.get("tok").is_none());
    }
}
