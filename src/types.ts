export interface CodeRange { file: string; startLine: number; endLine: number }
export interface Symbol { id: string; name: string; qualifiedName: string; kind: string; signature: string; range: CodeRange }
export interface FileEntry { path: string; language: string; size: number }
export interface GitInfo { available: boolean; branch: string | null; head: string | null }
export type AnchorStatus = 'exact' | 'moved' | 'changed' | 'missing' | 'ambiguous';
export type EvidenceRole = 'entry' | 'implementation' | 'constraint' | 'errorHandling' | 'test';
export interface AgentDeliveryInfo { deliveryId: string; summary: string; evidenceDigest: string; matchesSource: boolean }
export interface AnchorBinding { id: string; kind: 'symbol' | 'range'; symbolId: string | null; symbolName: string | null; range: CodeRange; contentHash: string; status: AnchorStatus; note: string; role?: EvidenceRole; responsibility?: string }
export type ReviewState = 'unread' | 'reading' | 'verified' | 'needsReview';
export interface Feature { id: string; name: string; description: string; color: string; parentId: string | null; anchors: AnchorBinding[]; reviewState: ReviewState; reviewedAt: string | null; reviewedDigest: string | null; currentDigest: string; agentDelivery?: AgentDeliveryInfo }
export interface ProjectSnapshot { overview?: ProjectOverview; root: string; name: string; git: GitInfo; files: FileEntry[]; symbols: Symbol[]; features: Feature[]; warnings: string[] }
export interface FileContent { path: string; language: string; content: string; hash: string }
export interface FeatureDraft { id?: string | null; name: string; description: string; color: string; parentId?: string | null }
export interface BindRequest { featureId: string; path: string; startLine: number; endLine: number; symbolId?: string | null; note?: string | null; anchorId?: string | null }
export interface DiffLine { kind: 'add' | 'remove' | 'context'; text: string; oldLine: number | null; newLine: number | null }
export interface DiffHunk { oldStart: number; oldLines: number; newStart: number; newLines: number; header: string; lines: DiffLine[] }
export interface ChangedFile { path: string; oldPath: string | null; status: 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked'; additions: number; deletions: number; hunks: DiffHunk[]; affectedSymbols: string[]; featureIds: string[] }
export interface ChangeSummary { base: string; head: string; files: ChangedFile[]; warnings: string[] }
export interface FileDiff { path: string; oldPath: string | null; oldContent: string; newContent: string; language: string }
export interface CodeRelation {
  id: string;
  kind: 'call' | 'reference';
  sourceSymbolId: string | null;
  sourceName: string;
  targetSymbolId: string | null;
  targetName: string;
  site: CodeRange;
  siteColumn: number;
  target: CodeRange | null;
  resolution: 'resolved' | 'unresolved' | 'ambiguous' | 'external';
  origin: 'syntax' | 'lsp';
  detail: string;
}
export interface FeatureImpact { featureId: string; kind: 'direct' | 'possible'; reason: string; evidence: CodeRelation[] }
export interface RelationEngine { name: string; status: 'ready' | 'partial' | 'unavailable'; detail: string }
export interface SymbolRelations {
  symbol: Symbol;
  sourceDigest: string;
  fileHashes: Record<string, string>;
  incoming: CodeRelation[];
  outgoing: CodeRelation[];
  references: CodeRelation[];
  changedSymbolIds: string[];
  impacts: FeatureImpact[];
  engine: RelationEngine;
  base: string;
  head: string;
  warnings: string[];
}


export interface OverviewModule { id: string; name: string; responsibility: string }
export interface ReadingStep { id: string; title: string; description: string; moduleId: string; featureId: string; evidenceId: string }
export interface ReadingPath { id: string; name: string; description: string; steps: ReadingStep[] }
export interface ProjectOverview { summary: string; modules: OverviewModule[]; paths: ReadingPath[] }
