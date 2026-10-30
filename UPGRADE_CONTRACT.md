# CodeTrail v0.2 integration contract

Goal: evidence-backed one-hop change navigation, first for Python. Preserve all v0.1 workflows and metadata. The product remains source-read-only. A language server is optional; no project build/test/macro execution, no shell interpolation or repository-provided executable configuration.

## Components

- `crates/reader-core/src/relations.rs`: captured-source analysis, Python bindings, evidence and one-hop impact.
- `crates/reader-core/src/semantic.rs`: optional bounded Python LSP definition/reference adapter.
- `crates/reader-core/src/types.rs` and `src/types.ts`: matching serialized DTOs.
- `src-tauri/src/lib.rs` and `crates/reader-cli/src/main.rs`: desktop and CLI command integration.
- `src/components/RelationsPane.tsx` and `src/App.tsx`: relationship presentation and guarded source navigation.
- `src/api.ts` and `src/demo.ts`: real desktop transport and explicitly labeled browser demonstration.

## DTOs

All fields camelCase; line ranges are 1-based inclusive. `siteColumn` is 1-based UTF-16 for LSP/Monaco interoperability.

```ts
interface CodeRelation {
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
interface FeatureImpact {
  featureId: string;
  kind: 'direct' | 'possible';
  reason: string;
  evidence: CodeRelation[];
}
interface RelationEngine {
  name: string;
  status: 'ready' | 'partial' | 'unavailable';
  detail: string;
}
interface SymbolRelations {
  symbol: Symbol;
  sourceDigest: string;
  fileHashes: Record<string, string>;
  incoming: CodeRelation[];
  outgoing: CodeRelation[];
  references: CodeRelation[]; // references to selected symbol that are not direct calls
  changedSymbolIds: string[];
  impacts: FeatureImpact[];
  engine: RelationEngine;
  base: string;
  head: string;
  warnings: string[];
}
```

## API

Core: `pub fn relations::analyze(root: &Path, symbol_id: &str, base: &str, head: &str, semantic: bool) -> anyhow::Result<SymbolRelations>`.

Frontend: `api.getRelations(symbolId: string, base: string, head: string, semantic?: boolean): Promise<SymbolRelations>`.

Tauri: `get_relations {symbolId, base, head, semantic}` uses opened session root. CLI: `getRelations` same payload. Only WORKTREE relation graph is supported. Historical comparisons must explicitly say relation graph is current-worktree-only, and must not infer historical impact from it. Deleted/renamed old symbols without a current identity have unknown historical callers.

Core scans current source, returns symbol and evidence from the same captured content, stable relation ids and digest. Revalidate content fingerprint before accepting semantic enrichment; discard stale results. Source changes during evidence navigation should be detected by comparing `fileHashes[path]` with `readFile(path).hash`; UI asks refresh instead of navigating stale locations.

## Conservative resolution

Start with Python static definitions, simple imports/aliases and unambiguous direct calls. Handle shadowing, duplicate module basenames, methods/dynamic receivers and function-as-value references conservatively. Never connect by globally matching only the name. Call expressions with unknown targets remain visible with a reason; language coverage is shown, including unsupported languages. References are not calls.

Syntax-resolved relations mean source binding, not runtime proof. Optional Pyright enrichment uses actual definition/reference results and reports availability/errors. It must not invent capabilities or hide unknowns. A missing server leaves static relationships working. The bounded adapter uses stdio JSON-RPC without a shell, analyzes an isolated captured-source workspace, and preserves explicit invalidation supplied by the static analyzer.

## Impact/UI

Use changed symbol ids from existing Git analysis. A direct caller of a changed symbol can create `possible` impact only with an actual resolved call edge and corresponding Feature anchors. Do not propagate impact forward to unchanged callees. Direct impact takes priority. Do not modify Feature.reviewState merely because a dependency may have changed.

UI: new Chinese relation view, one-hop incoming/outgoing lists and compact clickable local relationship graph; separate non-call references, unresolved targets, engine/source status and warnings. Clicking a relationship site or target reveals real source. Mark changed nodes and possible Feature impact; graph is local, not an alleged complete execution flow. Semantic enhancement is opt-in, Python-first and can be unavailable. Demo must contain actual corresponding call sites and a real relation fixture, not decorative fabricated edges.

## Acceptance

- A->B->C, B changed: B direct, A possible, C not automatically impacted.
- Different modules with same names/import aliases/shadowing don't get incorrectly joined.
- Passing function as a value is reference, not call.
- Dynamic/external/missing targets are visible; every resolved edge opens real evidence.
- Current/historical revision boundary and stale semantic/evidence responses are explicit.
- Preserve the original indexing, Git, annotation and review behaviors. The final executed regression counts and platform boundaries are recorded in `docs/VALIDATION.md`.
