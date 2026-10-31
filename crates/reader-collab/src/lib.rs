//! Version-bound collaboration. Source code and private annotations stay local.
pub mod client;
pub mod server;
pub mod service;
pub mod tasks;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Version {
    pub commit: String,
    pub source_digest: String,
    pub association_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scope {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeatureScope {
    pub id: String,
    pub name: String,
    pub evidence: Vec<Scope>,
    pub modules: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub version: Version,
    pub features: Vec<FeatureScope>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub name: String,
    pub role: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub token_hash: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentInput {
    pub id: String,
    pub body: String,
    pub feature_id: Option<String>,
    pub scope: Option<Scope>,
    pub parent_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub input: CommentInput,
    pub author_id: String,
    pub created_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskInput {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub acceptance: String,
    pub boundary: String,
    pub comment_ids: Vec<String>,
    pub feature_ids: Vec<String>,
    pub scopes: Vec<Scope>,
    pub contracts: Vec<String>,
    pub owner_id: Option<String>,
    pub depends_on: Vec<String>,
    pub coupling_note: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub input: TaskInput,
    pub status: String,
    pub revision: u64,
    pub result_commit: Option<String>,
    pub review_note: Option<String>,
    #[serde(default)]
    pub merged_into: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    pub left: String,
    pub right: String,
    pub kind: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub action: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub data: Value,
}
