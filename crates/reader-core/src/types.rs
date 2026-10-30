use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeRange {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub signature: String,
    pub range: CodeRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub language: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub available: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AnchorKind {
    Symbol,
    Range,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AnchorStatus {
    Exact,
    Moved,
    Changed,
    Missing,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnchorBinding {
    pub id: String,
    pub kind: AnchorKind,
    pub symbol_id: Option<String>,
    pub symbol_name: Option<String>,
    pub range: CodeRange,
    pub content_hash: String,
    pub status: AnchorStatus,
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<EvidenceRole>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub responsibility: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceRole {
    Entry,
    Implementation,
    Constraint,
    ErrorHandling,
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDeliveryInfo {
    pub delivery_id: String,
    pub summary: String,
    pub evidence_digest: String,
    #[serde(default)]
    pub matches_source: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReviewState {
    Unread,
    Reading,
    Verified,
    NeedsReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub id: String,
    pub name: String,
    pub description: String,
    pub color: String,
    pub parent_id: Option<String>,
    pub anchors: Vec<AnchorBinding>,
    pub review_state: ReviewState,
    pub reviewed_at: Option<String>,
    pub reviewed_digest: Option<String>,
    /// Recomputed from current source and annotations on every project read.
    #[serde(default)]
    pub current_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_delivery: Option<AgentDeliveryInfo>,
}

/// Agent-authored reading guide; sequence is explanatory, not a verified call graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectOverview {
    pub summary: String,
    pub modules: Vec<OverviewModule>,
    pub paths: Vec<ReadingPath>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverviewModule {
    pub id: String,
    pub name: String,
    pub responsibility: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingPath {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<ReadingStep>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingStep {
    pub id: String,
    pub title: String,
    pub description: String,
    pub module_id: String,
    pub feature_id: String,
    pub evidence_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<ProjectOverview>,
    pub root: String,
    pub name: String,
    pub git: GitInfo,
    pub files: Vec<FileEntry>,
    pub symbols: Vec<Symbol>,
    pub features: Vec<Feature>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub path: String,
    pub language: String,
    pub content: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureDraft {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub color: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindRequest {
    pub feature_id: String,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(default)]
    pub symbol_id: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub anchor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiffLineKind {
    Add,
    Remove,
    Context,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChangeStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<DiffHunk>,
    pub affected_symbols: Vec<String>,
    pub feature_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSummary {
    pub base: String,
    pub head: String,
    pub files: Vec<ChangedFile>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub old_content: String,
    pub new_content: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RelationKind {
    Call,
    Reference,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RelationResolution {
    Resolved,
    Unresolved,
    Ambiguous,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RelationOrigin {
    Syntax,
    Lsp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeRelation {
    pub id: String,
    pub kind: RelationKind,
    pub source_symbol_id: Option<String>,
    pub source_name: String,
    pub target_symbol_id: Option<String>,
    pub target_name: String,
    pub site: CodeRange,
    pub site_column: u32,
    pub target: Option<CodeRange>,
    pub resolution: RelationResolution,
    pub origin: RelationOrigin,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FeatureImpactKind {
    Direct,
    Possible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeatureImpact {
    pub feature_id: String,
    pub kind: FeatureImpactKind,
    pub reason: String,
    pub evidence: Vec<CodeRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RelationEngineStatus {
    Ready,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelationEngine {
    pub name: String,
    pub status: RelationEngineStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SymbolRelations {
    pub symbol: Symbol,
    pub source_digest: String,
    pub file_hashes: std::collections::BTreeMap<String, String>,
    pub incoming: Vec<CodeRelation>,
    pub outgoing: Vec<CodeRelation>,
    pub references: Vec<CodeRelation>,
    pub changed_symbol_ids: Vec<String>,
    pub impacts: Vec<FeatureImpact>,
    pub engine: RelationEngine,
    pub base: String,
    pub head: String,
    pub warnings: Vec<String>,
}
