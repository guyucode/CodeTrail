# CodeTrail Reader foundational integration contract

All IPC JSON uses camelCase. Code ranges are 1-based inclusive lines. Root owns src/types.ts, src/api.ts, src/demo.ts, manifests, Tauri bridge, integration tests and documentation. No network/model calls in the product; no source writes. Annotation metadata is stored only in .reader/features and .reader/index.json. The v0.2 relation and optional isolated Python LSP interfaces are defined in UPGRADE_CONTRACT.md; no project code is executed.

## DTOs (Rust use serde rename_all="camelCase")

CodeRange { file: string, startLine: number, endLine: number }
Symbol { id, name, qualifiedName, kind, signature: string, range: CodeRange }
FileEntry { path, language: string, size: number }
GitInfo { available: boolean, branch: string|null, head: string|null }
AnchorBinding { id: string, kind: "symbol"|"range", symbolId: string|null, symbolName: string|null, range: CodeRange, contentHash: string, status: "exact"|"moved"|"changed"|"missing"|"ambiguous", note: string }
ReviewState = "unread" | "reading" | "verified" | "needsReview"
Feature { id, name, description, color: string, parentId: string|null, anchors: AnchorBinding[], reviewState: ReviewState, reviewedAt: string|null, reviewedDigest: string|null, currentDigest: string }
ProjectSnapshot { root, name: string, git: GitInfo, files: FileEntry[], symbols: Symbol[], features: Feature[], warnings: string[] }
FileContent { path, language, content, hash: string }
FeatureDraft { id?: string|null, name, description, color: string, parentId?: string|null }
BindRequest { featureId, path: string, startLine, endLine: number, symbolId?: string|null, note?: string|null, anchorId?: string|null } // anchorId replaces an existing binding, preserves id
DiffLine { kind: "add"|"remove"|"context", text: string, oldLine: number|null, newLine: number|null }
DiffHunk { oldStart, oldLines, newStart, newLines: number, header: string, lines: DiffLine[] }
ChangedFile { path: string, oldPath: string|null, status: "modified"|"added"|"deleted"|"renamed"|"untracked", additions, deletions: number, hunks: DiffHunk[], affectedSymbols: string[], featureIds: string[] }
ChangeSummary { base, head: string, files: ChangedFile[], warnings: string[] }
FileDiff { path: string, oldPath: string|null, oldContent, newContent, language: string }

## Rust reader-core public interface (Result<T> = anyhow::Result<T>)

pub mod types; pub use types::*; pub mod git;
pub fn open_project(root: &Path) -> Result<ProjectSnapshot>;
pub fn read_file(root: &Path, path: &str) -> Result<FileContent>;
pub fn save_feature(root: &Path, draft: FeatureDraft) -> Result<ProjectSnapshot>;
pub fn delete_feature(root: &Path, feature_id: &str) -> Result<ProjectSnapshot>;
pub fn bind_anchor(root: &Path, request: BindRequest) -> Result<ProjectSnapshot>;
pub fn remove_anchor(root: &Path, feature_id: &str, anchor_id: &str) -> Result<ProjectSnapshot>;
pub fn set_review(root: &Path, feature_id: &str, state: ReviewState, expected_digest: Option<&str>) -> Result<ProjectSnapshot>;
pub fn export_review(root: &Path) -> Result<String>; // returns Markdown, never writes source

Git module (standalone for parallel development, uses crate::types):
pub fn info(root: &Path) -> GitInfo;
pub fn changes(root: &Path, base: &str, head: &str, snapshot: &ProjectSnapshot) -> Result<ChangeSummary>;
pub fn file_diff(root: &Path, path: &str, base: &str, head: &str) -> Result<FileDiff>;
HEAD or a commit/ref accepted as base; WORKTREE or commit/ref as head. WORKTREE compares the base against final current files, including staged, unstaged and untracked changes. Must safely handle non-Git and unborn repositories. Must include deleted files and old/new ranges. No shell interpolation; use Command git args, disable external diff/textconv, verify revisions, disallow leading options and traversals. Respect subdirectory root boundaries. Feature/symbol matching based on changed ranges, not arbitrary all-file assignments. Mark approximate old-side associations in warnings where relevant.

## Frontend API surface (root implements)

api.isDesktop: boolean
api.chooseProject(): Promise<ProjectSnapshot|null>
api.openProject(path: string): Promise<ProjectSnapshot>
api.refresh(): Promise<ProjectSnapshot>
api.readFile(path: string): Promise<FileContent>
api.saveFeature(draft: FeatureDraft): Promise<ProjectSnapshot>
api.deleteFeature(featureId: string): Promise<ProjectSnapshot>
api.bindAnchor(request: BindRequest): Promise<ProjectSnapshot>
api.removeAnchor(featureId: string, anchorId: string): Promise<ProjectSnapshot>
api.setReview(featureId: string, state: ReviewState, expectedDigest?: string): Promise<ProjectSnapshot>
api.getChanges(base: string, head: string): Promise<ChangeSummary>
api.getFileDiff(path: string, base: string, head: string): Promise<FileDiff>
api.exportReview(): Promise<string>
api.loadDemo(): Promise<ProjectSnapshot>

Browser mode runs a clearly-labelled bundled demo using localStorage metadata. Desktop uses Tauri invoke. UI starts with demo and shows an obvious Open repository button. Use @monaco-editor/react with LOCAL monaco-editor instance/worker, never CDN; readOnly and originalEditable=false. All views Chinese. Lucide icons available. Vite/React/TypeScript. User actions include real feature create/edit, selection binding, mark reviewed, jump anchors, cursor reverse lookup, search/filter, unified/side-by-side diff, refresh, export review. Selecting a Feature shows anchors grouped by file; don't pretend concatenated fragments are one continuous source file. Surface stale anchors visibly. No fake AI chat or decorative nonfunctional buttons.

Verified requires expectedDigest matching the feature currentDigest in the last displayed ProjectSnapshot. Refresh recomputes currentDigest from current source; persisted currentDigest is never trusted. A mismatch fails rather than marking unseen changed content reviewed.
