import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDownLeft,
  ArrowUpRight,
  CheckCircle2,
  ChevronRight,
  CircleDot,
  FileCode2,
  GitBranch,
  Info,
  Link2,
  Loader2,
  Network,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  TriangleAlert,
} from "lucide-react";
import { api } from "../api";
import type {
  CodeRange,
  CodeRelation,
  Feature,
  FileContent,
  ProjectSnapshot,
  SymbolRelations,
} from "../types";

export interface RelationNavigation {
  range: CodeRange;
  column: number;
  expectedHash: string;
  content: FileContent;
  symbolId: string;
  symbolName: string;
}

interface Props {
  project: ProjectSnapshot;
  symbolId: string | null;
  base: string;
  head: string;
  revision: number;
  onSelectSymbol: (id: string) => void;
  onNavigate: (navigation: RelationNavigation) => void;
  onSelectFeature: (id: string) => void;
  onUseWorktree: () => void;
}

type GraphNode = {
  id: string;
  name: string;
  range: CodeRange;
  column: number;
  changed: boolean;
  count: number;
};

const RESOLUTION = {
  resolved: "已解析",
  unresolved: "未解析",
  ambiguous: "有歧义",
  external: "外部边界",
};
const REVIEW_LABEL = {
  unread: "未阅读",
  reading: "阅读中",
  verified: "已确认",
  needsReview: "待复核",
};

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function location(range: CodeRange) {
  return `${range.file}:L${range.startLine}${range.endLine === range.startLine ? "" : `–${range.endLine}`}`;
}

function uniqueRelations(relations: CodeRelation[]) {
  return Array.from(
    new Map(relations.map((relation) => [relation.id, relation])).values(),
  );
}

function graphNodes(
  relations: CodeRelation[],
  side: "incoming" | "outgoing",
  changed: Set<string>,
) {
  const grouped = new Map<string, GraphNode>();
  for (const relation of relations) {
    const incoming = side === "incoming";
    const range = incoming ? relation.site : relation.target;
    if (!range) continue;
    const symbolId = incoming
      ? relation.sourceSymbolId
      : relation.targetSymbolId;
    const name = incoming ? relation.sourceName : relation.targetName;
    const id = symbolId || `${name}:${range.file}`;
    const existing = grouped.get(id);
    if (existing) existing.count += 1;
    else
      grouped.set(id, {
        id,
        name,
        range,
        column: incoming ? relation.siteColumn : 1,
        changed: !!symbolId && changed.has(symbolId),
        count: 1,
      });
  }
  return Array.from(grouped.values());
}

export default function RelationsPane({
  project,
  symbolId,
  base,
  head,
  revision,
  onSelectSymbol,
  onNavigate,
  onSelectFeature,
  onUseWorktree,
}: Props) {
  const selected =
    project.symbols.find((symbol) => symbol.id === symbolId) || null;
  const language = project.files.find(
    (file) => file.path === selected?.range.file,
  )?.language;
  const python = language === "python";
  const historical = head !== "WORKTREE";
  const [semantic, setSemantic] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [result, setResult] = useState<{
    key: string;
    data: SymbolRelations;
  } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [navigationError, setNavigationError] = useState("");
  const [navigating, setNavigating] = useState("");
  const navigationSequence = useRef(0);
  const requestKey = JSON.stringify([
    project.root,
    symbolId,
    base,
    head,
    revision,
    refresh,
    semantic,
  ]);
  const data = result?.key === requestKey ? result.data : null;
  const symbols = useMemo(
    () =>
      [...project.symbols].sort(
        (a, b) =>
          a.range.file.localeCompare(b.range.file) ||
          a.range.startLine - b.range.startLine,
      ),
    [project.symbols],
  );

  useEffect(() => {
    let cancelled = false;
    navigationSequence.current += 1;
    setNavigationError("");
    setNavigating("");
    setResult(null);
    setError("");
    if (!symbolId || !python || historical) {
      setLoading(false);
      return;
    }
    setLoading(true);
    api
      .getRelations(symbolId, base, head, semantic)
      .then((value) => {
        if (!cancelled) setResult({ key: requestKey, data: value });
      })
      .catch((reason) => {
        if (!cancelled) setError(errorMessage(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
      navigationSequence.current += 1;
    };
  }, [requestKey, symbolId, python, historical, base, head, semantic]);

  async function navigate(range: CodeRange, column = 1) {
    if (!data) return;
    const token = ++navigationSequence.current;
    const expectedHash = data.fileHashes[range.file];
    setNavigationError("");
    if (!expectedHash) {
      setNavigationError(
        `此快照没有 ${range.file} 的内容指纹，请刷新关系后重试。`,
      );
      return;
    }
    setNavigating(location(range));
    try {
      const content = await api.readFile(range.file);
      if (navigationSequence.current !== token) return;
      if (content.hash !== expectedHash) {
        throw new Error(
          `${range.file} 已在分析后变化。已停止导航，请刷新关系后再打开证据。`,
        );
      }
      onNavigate({
        range,
        column,
        expectedHash,
        content,
        symbolId: data.symbol.id,
        symbolName: data.symbol.name,
      });
    } catch (reason) {
      if (navigationSequence.current === token)
        setNavigationError(errorMessage(reason));
    } finally {
      if (navigationSequence.current === token) setNavigating("");
    }
  }

  const changed = new Set(data?.changedSymbolIds || []);
  const incoming =
    data?.incoming.filter(
      (relation) =>
        relation.kind === "call" && relation.resolution === "resolved",
    ) || [];
  const outgoing =
    data?.outgoing.filter(
      (relation) =>
        relation.kind === "call" && relation.resolution === "resolved",
    ) || [];
  const references =
    data?.references.filter((relation) => relation.kind === "reference") || [];
  const boundaries = data
    ? uniqueRelations([...data.incoming, ...data.outgoing]).filter(
        (relation) => relation.resolution !== "resolved",
      )
    : [];
  const incomingNodes = graphNodes(incoming, "incoming", changed);
  const outgoingNodes = graphNodes(outgoing, "outgoing", changed);

  return (
    <section className="relations-view" aria-label="相关逻辑视图">
      <div className="relations-heading">
        <div>
          <span className="eyebrow">FOLLOW THE EVIDENCE</span>
          <h2>沿着调用，理解改动</h2>
          <p>围绕一个符号查看一跳关系，每条连接都有源码证据。</p>
        </div>
        <span className="relations-scope">
          <Network size={14} /> Python · 一跳
        </span>
      </div>

      <div className="relation-controls">
        <label className="relation-symbol-picker">
          <span>当前符号</span>
          <select
            aria-label="分析关系的符号"
            value={symbolId || ""}
            onChange={(event) => onSelectSymbol(event.target.value)}
          >
            {!selected && <option value="">选择一个符号</option>}
            {symbols.map((symbol) => (
              <option key={symbol.id} value={symbol.id}>
                {symbol.name} · {symbol.range.file}:L{symbol.range.startLine}
              </option>
            ))}
          </select>
        </label>
        <button
          className="button button-subtle"
          aria-label="刷新关系"
          disabled={loading || !selected || !python || historical}
          onClick={() => setRefresh((value) => value + 1)}
        >
          <RefreshCw size={13} className={loading ? "spin" : ""} />
          刷新
        </button>
      </div>

      {historical ? (
        <div className="relation-state relation-historical" role="status">
          <GitBranch size={28} />
          <h3>历史差异与当前关系分开阅读</h3>
          <p>
            当前对比为{" "}
            <code>
              {base} → {head}
            </code>
            。关系分析只读取当前工作区，不能据此推断历史调用或历史影响。
          </p>
          <p>已删除、已重命名且没有当前身份的旧符号，其历史调用方仍属未知。</p>
          <button className="button button-subtle" onClick={onUseWorktree}>
            分析当前工作区
          </button>
        </div>
      ) : !selected ? (
        <div className="relation-state">
          <Network size={30} />
          <h3>选择要理解的函数或类</h3>
          <p>在上方选择符号，或在源码中定位后打开“相关逻辑”。</p>
        </div>
      ) : !python ? (
        <div className="relation-state relation-unsupported" role="status">
          <FileCode2 size={30} />
          <h3>此语言暂未提供关系分析</h3>
          <p>
            当前符号位于 {selected.range.file}。本版本支持 Python
            的静态调用与引用；其他语言仍可阅读、绑定功能和查看差异。
          </p>
        </div>
      ) : loading ? (
        <div className="relation-state" role="status">
          <Loader2 className="spin" size={28} />
          <h3>
            {semantic ? "正在核验 Python 语义证据" : "正在读取当前代码关系"}
          </h3>
          <p>解析调用位置、符号绑定和直接依赖。</p>
        </div>
      ) : error ? (
        <div className="relation-state relation-error" role="alert">
          <TriangleAlert size={28} />
          <h3>关系分析尚未完成</h3>
          <p>{error}</p>
          <button
            className="button button-subtle"
            onClick={() => setRefresh((value) => value + 1)}
          >
            <RefreshCw size={13} />
            重试分析
          </button>
        </div>
      ) : data ? (
        <>
          <div className="relation-snapshot">
            <div className="relation-engine">
              <span className={`engine-dot engine-${data.engine.status}`} />
              <strong>{data.engine.name}</strong>
              <span className="engine-status">
                {data.engine.status === "ready"
                  ? "已就绪"
                  : data.engine.status === "partial"
                    ? "部分可用"
                    : "不可用"}
              </span>
            </div>
            <div className="snapshot-meta">
              <span>当前工作区</span>
              <span title={data.sourceDigest}>
                快照 {data.sourceDigest.slice(0, 10)}
              </span>
              <span>对比 {data.base} → WORKTREE</span>
            </div>
            <p>{data.engine.detail}</p>
            <div className="semantic-action">
              <button
                className="button button-subtle"
                onClick={() => {
                  setSemantic(true);
                  setRefresh((value) => value + 1);
                }}
                disabled={loading}
              >
                <Sparkles size={13} />
                {semantic ? "重新语义分析" : "Python 语义增强"}
              </button>
              <span>按需使用已安装的语言服务；不可用时仍保留静态结果。</span>
            </div>
          </div>

          {!!data.warnings.length && (
            <div className="relation-warnings" role="status">
              <TriangleAlert size={15} />
              <div>
                {data.warnings.map((warning, index) => (
                  <p key={index}>{warning}</p>
                ))}
              </div>
            </div>
          )}
          {navigationError && (
            <div className="relation-navigation-error" role="alert">
              <TriangleAlert size={16} />
              <p>{navigationError}</p>
              <button
                className="text-button"
                onClick={() => setRefresh((value) => value + 1)}
              >
                刷新关系
              </button>
            </div>
          )}
          {navigating && (
            <div className="relation-navigation-progress" role="status">
              <Loader2 size={13} className="spin" />
              正在核验 {navigating}
            </div>
          )}

          <LocalGraph
            data={data}
            incoming={incomingNodes}
            outgoing={outgoingNodes}
            changed={changed}
            disabled={!!navigating}
            onNavigate={navigate}
          />

          <div className="relation-call-columns">
            <RelationSection
              title="调用方"
              subtitle="哪些代码直接调用当前符号"
              icon={<ArrowDownLeft size={15} />}
              count={incoming.length}
              className="incoming-relations"
            >
              {incoming.length ? (
                incoming.map((relation) => (
                  <RelationRow
                    key={relation.id}
                    relation={relation}
                    emphasis="source"
                    disabled={!!navigating}
                    onNavigate={navigate}
                  />
                ))
              ) : (
                <p className="relation-empty">
                  当前快照未找到已解析的直接调用方。
                </p>
              )}
            </RelationSection>
            <RelationSection
              title="被调用方"
              subtitle="当前符号直接调用了哪些定义"
              icon={<ArrowUpRight size={15} />}
              count={outgoing.length}
              className="outgoing-relations"
            >
              {outgoing.length ? (
                outgoing.map((relation) => (
                  <RelationRow
                    key={relation.id}
                    relation={relation}
                    emphasis="target"
                    disabled={!!navigating}
                    onNavigate={navigate}
                  />
                ))
              ) : (
                <p className="relation-empty">
                  当前快照未找到已解析的直接调用目标。
                </p>
              )}
            </RelationSection>
          </div>

          <RelationSection
            title="非调用引用"
            subtitle="函数被作为值传递、保存或返回；这些位置不代表调用"
            icon={<Link2 size={15} />}
            count={references.length}
            className="reference-relations"
          >
            {references.length ? (
              references.map((relation) => (
                <RelationRow
                  key={relation.id}
                  relation={relation}
                  emphasis="source"
                  disabled={!!navigating}
                  onNavigate={navigate}
                />
              ))
            ) : (
              <p className="relation-empty">当前快照未发现非调用引用。</p>
            )}
          </RelationSection>

          <RelationSection
            title="动态与外部边界"
            subtitle="无法唯一绑定的目标保留为未知，不把同名定义连成调用"
            icon={<TriangleAlert size={15} />}
            count={boundaries.length}
            className="boundary-relations"
          >
            {boundaries.length ? (
              boundaries.map((relation) => (
                <RelationRow
                  key={relation.id}
                  relation={relation}
                  emphasis="target"
                  disabled={!!navigating}
                  onNavigate={navigate}
                />
              ))
            ) : (
              <p className="relation-empty">
                当前符号没有额外的未解析调用边界；这不表示运行时路径已被完整证明。
              </p>
            )}
          </RelationSection>

          <RelationSection
            title="功能影响线索"
            subtitle="直接改动与一跳依赖分别标记，按证据决定下一步阅读"
            icon={<GitBranch size={15} />}
            count={data.impacts.length}
            className="relation-impacts"
          >
            {data.impacts.length ? (
              data.impacts.map((impact) => {
                const feature = project.features.find(
                  (item) => item.id === impact.featureId,
                );
                return (
                  <div
                    key={`${impact.featureId}:${impact.kind}`}
                    className={`relation-impact impact-${impact.kind}`}
                    data-feature-id={impact.featureId}
                  >
                    <div className="relation-impact-heading">
                      <span className={`impact-badge ${impact.kind}`}>
                        {impact.kind === "direct" ? "直接改动" : "可能受影响"}
                      </span>
                      <button
                        className="relation-feature-link"
                        disabled={!feature}
                        onClick={() => onSelectFeature(impact.featureId)}
                      >
                        {feature?.name || "未找到功能"}
                        <ChevronRight size={12} />
                      </button>
                      {feature && <ReviewState feature={feature} />}
                    </div>
                    <p className="impact-reason">{impact.reason}</p>
                    {impact.evidence.length > 0 && (
                      <div className="impact-evidence">
                        {uniqueRelations(impact.evidence).map((relation) => (
                          <button
                            key={relation.id}
                            disabled={!!navigating}
                            onClick={() =>
                              navigate(relation.site, relation.siteColumn)
                            }
                            aria-label={`查看影响证据：${relation.sourceName} 调用 ${relation.targetName}`}
                          >
                            <FileCode2 size={12} />
                            <span>
                              {relation.sourceName} → {relation.targetName}
                            </span>
                            <code>{location(relation.site)}</code>
                            <ArrowUpRight size={11} />
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })
            ) : (
              <p className="relation-empty">
                未找到与本次改动对应的功能影响证据。
              </p>
            )}
            <p className="impact-safety-note">
              <ShieldCheck size={13} />
              “可能受影响”仅提供阅读线索，不会改变功能的审阅状态，也不向未改动的被调用方继续传播。
            </p>
          </RelationSection>

          <div className="relation-footnote">
            <Info size={14} />
            <p>
              静态解析确认的是源码中的绑定关系，不是运行时执行证明。此处只展示当前符号的一跳范围。
            </p>
          </div>
        </>
      ) : null}
    </section>
  );
}

function ReviewState({ feature }: { feature: Feature }) {
  return (
    <span className={`impact-review review-${feature.reviewState}`}>
      {feature.reviewState === "verified" ? (
        <CheckCircle2 size={12} />
      ) : (
        <CircleDot size={12} />
      )}
      {REVIEW_LABEL[feature.reviewState]}
    </span>
  );
}

function RelationSection({
  title,
  subtitle,
  icon,
  count,
  className,
  children,
}: {
  title: string;
  subtitle: string;
  icon: React.ReactNode;
  count: number;
  className: string;
  children: React.ReactNode;
}) {
  return (
    <section className={`relation-section ${className}`} aria-label={title}>
      <div className="relation-section-heading">
        <div>
          <h3>
            {icon}
            {title}
            <span>{count}</span>
          </h3>
          <p>{subtitle}</p>
        </div>
      </div>
      <div className="relation-section-body">{children}</div>
    </section>
  );
}

function RelationRow({
  relation,
  emphasis,
  disabled,
  onNavigate,
}: {
  relation: CodeRelation;
  emphasis: "source" | "target";
  disabled: boolean;
  onNavigate: (range: CodeRange, column?: number) => void;
}) {
  const resolved = relation.resolution === "resolved";
  return (
    <article
      className={`relation-row resolution-${relation.resolution}`}
      data-relation-id={relation.id}
    >
      <div className="relation-row-title">
        <code>
          {emphasis === "source" ? relation.sourceName : relation.targetName}
        </code>
        <span className={`resolution-badge ${relation.resolution}`}>
          {relation.kind === "reference" && resolved
            ? "引用"
            : RESOLUTION[relation.resolution]}
        </span>
      </div>
      <p className="relation-detail">{relation.detail}</p>
      <div className="relation-site">
        <FileCode2 size={12} />
        <code>{location(relation.site)}</code>
        <span>{relation.origin === "lsp" ? "语义证据" : "语法证据"}</span>
      </div>
      <div className="relation-row-actions">
        <button
          disabled={disabled}
          onClick={() => onNavigate(relation.site, relation.siteColumn)}
          aria-label={`查看${relation.kind === "call" ? "调用" : "引用"}位置：${relation.sourceName} → ${relation.targetName}`}
        >
          <ArrowUpRight size={12} />
          {relation.kind === "call" ? "调用位置" : "引用位置"}
        </button>
        {resolved && relation.target && (
          <button
            disabled={disabled}
            onClick={() => onNavigate(relation.target!)}
            aria-label={`查看定义：${relation.targetName}`}
          >
            <CodeDefinition />
            目标定义
          </button>
        )}
      </div>
    </article>
  );
}

function CodeDefinition() {
  return (
    <span className="definition-icon" aria-hidden="true">
      ƒ
    </span>
  );
}

function LocalGraph({
  data,
  incoming,
  outgoing,
  changed,
  disabled,
  onNavigate,
}: {
  data: SymbolRelations;
  incoming: GraphNode[];
  outgoing: GraphNode[];
  changed: Set<string>;
  disabled: boolean;
  onNavigate: (range: CodeRange, column?: number) => void;
}) {
  const shownIncoming = incoming.slice(0, 4);
  const shownOutgoing = outgoing.slice(0, 4);
  const height = Math.max(
    180,
    Math.max(shownIncoming.length, shownOutgoing.length) * 70 + 24,
  );
  const position = (index: number, count: number) =>
    height / 2 - ((count - 1) * 70) / 2 + index * 70;
  const graphNode = (node: GraphNode, side: "incoming" | "outgoing") => (
    <button
      key={node.id}
      className={`relation-graph-node ${node.changed ? "node-changed" : ""}`}
      disabled={disabled}
      onClick={() => onNavigate(node.range, node.column)}
      aria-label={`图节点：${node.name}，${side === "incoming" ? "查看调用证据" : "查看定义"}`}
      title={`${node.name} · ${location(node.range)}`}
    >
      <span className="graph-node-name">
        {node.changed && <span className="node-change-dot" />}
        <code>{node.name}</code>
        <ArrowUpRight size={11} />
      </span>
      <span className="graph-node-meta">
        {node.range.file.split("/").at(-1)}
        {node.count > 1 ? ` · ${node.count} 处调用` : ""}
      </span>
      {node.changed && <span className="graph-change-label">代码有改动</span>}
    </button>
  );

  return (
    <section className="relation-graph" aria-label="局部调用图">
      <div className="relation-graph-heading">
        <h3>
          <Network size={15} />
          局部调用图
        </h3>
        <span>实线仅表示已解析的直接调用</span>
      </div>
      <div className="relation-graph-scroll">
        <div className="graph-column-labels">
          <span>调用方</span>
          <span>当前符号</span>
          <span>被调用方</span>
        </div>
        <div className="relation-graph-canvas" style={{ height }}>
          <svg
            className="relation-connectors"
            viewBox={`0 0 1000 ${height}`}
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            <defs>
              <marker
                id="relation-arrow"
                viewBox="0 0 10 10"
                refX="9"
                refY="5"
                markerWidth="5"
                markerHeight="5"
                orient="auto-start-reverse"
              >
                <path d="M 0 0 L 10 5 L 0 10 z" />
              </marker>
            </defs>
            {shownIncoming.map((node, index) => (
              <path
                key={node.id}
                className={
                  node.changed || changed.has(data.symbol.id)
                    ? "edge-changed"
                    : ""
                }
                d={`M 290 ${position(index, shownIncoming.length)} C 340 ${position(index, shownIncoming.length)}, 340 ${height / 2}, 387 ${height / 2}`}
                markerEnd="url(#relation-arrow)"
              />
            ))}
            {shownOutgoing.map((node, index) => (
              <path
                key={node.id}
                className={node.changed ? "edge-changed" : ""}
                d={`M 610 ${height / 2} C 660 ${height / 2}, 660 ${position(index, shownOutgoing.length)}, 707 ${position(index, shownOutgoing.length)}`}
                markerEnd="url(#relation-arrow)"
              />
            ))}
          </svg>
          <div className="graph-column incoming-nodes">
            {shownIncoming.length ? (
              shownIncoming.map((node) => graphNode(node, "incoming"))
            ) : (
              <span className="graph-empty">未发现直接调用方</span>
            )}
          </div>
          <div className="graph-column center-node">
            <button
              className={`relation-graph-node graph-current ${changed.has(data.symbol.id) ? "node-changed" : ""}`}
              disabled={disabled}
              onClick={() => onNavigate(data.symbol.range)}
              aria-label={`图节点：${data.symbol.name}，查看当前定义`}
            >
              <span className="graph-node-name">
                <code>{data.symbol.name}</code>
              </span>
              <span className="graph-node-meta">
                {data.symbol.kind} · 当前符号
              </span>
              {changed.has(data.symbol.id) && (
                <span className="graph-change-label">代码有改动</span>
              )}
            </button>
          </div>
          <div className="graph-column outgoing-nodes">
            {shownOutgoing.length ? (
              shownOutgoing.map((node) => graphNode(node, "outgoing"))
            ) : (
              <span className="graph-empty">未发现已解析目标</span>
            )}
          </div>
        </div>
      </div>
      <div className="graph-legend">
        <span>
          <i />
          本次对比中改动的符号
        </span>
        <span>
          点击节点打开已核验的源码
          {incoming.length > 4 || outgoing.length > 4
            ? "；图中每侧最多 4 个节点，其余见下方列表"
            : ""}
        </span>
      </div>
    </section>
  );
}
