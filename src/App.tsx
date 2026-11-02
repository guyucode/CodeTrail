import CodeSearch from "./components/CodeSearch";
import RuntimePane, { type RuntimeLocation } from "./components/RuntimePane";
import WorkPanel from "./components/WorkPanel";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Editor, {
  DiffEditor,
  type DiffOnMount,
  type OnMount,
} from "@monaco-editor/react";
import type { editor, IDisposable } from "monaco-editor";
import {
  ArrowUpRight,
  BookOpen,
  Check,
  CheckCircle2,
  ChevronRight,
  Circle,
  CircleDot,
  Code2,
  Copy,
  Download,
  FileCode2,
  FolderOpen,
  GitBranch,
  GitCompareArrows,
  Layers,
  Link2,
  Loader2,
  Map,
  MoreHorizontal,
  Network,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  ShieldCheck,
  SplitSquareHorizontal,
  Trash2,
  TriangleAlert,
  Unlink,
  X,
} from "lucide-react";
import { api } from "./api";
import RelationsPane, {
  type RelationNavigation,
} from "./components/RelationsPane";
import type {
  AnchorBinding,
  ChangeSummary,
  ChangedFile,
  CodeRange,
  Feature,
  FeatureDraft,
  FileContent,
  FileDiff,
  ProjectSnapshot,
  ReviewState,
  Symbol as CodeSymbol,
} from "./types";
import "./styles.css";
import ProjectOverview from "./components/ProjectOverview";

type View = "source" | "diff" | "relations" | "overview" | "runs";
type NavTab = "features" | "files" | "changes";
type Jump = {
  file: string;
  startLine: number;
  endLine: number;
  stamp: number;
  column?: number;
  select?: boolean;
  expectedHash?: string;
};
type Selection = { startLine: number; endLine: number; empty: boolean };
type FeatureForm = {
  id?: string;
  name: string;
  description: string;
  color: string;
  parentId: string;
};
type BindForm = {
  featureId: string;
  range: CodeRange;
  symbol: CodeSymbol | null;
  note: string;
  anchorId?: string;
};

const COLORS = [
  "#a78bfa",
  "#67c9f3",
  "#5ed5b0",
  "#f3be69",
  "#f08dac",
  "#a6b4d0",
];
const REVIEW: Record<ReviewState, { text: string; className: string }> = {
  unread: { text: "未读", className: "unread" },
  reading: { text: "阅读中", className: "reading" },
  verified: { text: "已读", className: "verified" },
  needsReview: { text: "需重读", className: "needs-review" },
};
const ANCHOR_STATUS: Record<AnchorBinding["status"], string> = {
  exact: "定位正常",
  moved: "位置已更新",
  changed: "内容已变化",
  missing: "锚点失效",
  ambiguous: "定位有歧义",
};
const CHANGE_STATUS: Record<ChangedFile["status"], string> = {
  added: "新增",
  modified: "修改",
  deleted: "删除",
  renamed: "重命名",
  untracked: "未跟踪",
};

function message(error: unknown): string {
  return error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "操作未完成，请重试。";
}

function filename(path: string) {
  return path.split("/").at(-1) || path;
}
function dirname(path: string) {
  return path.includes("/")
    ? path.slice(0, path.lastIndexOf("/"))
    : "项目根目录";
}
function safeColor(color: string) {
  return /^#[a-fA-F0-9]{6}$/.test(color) ? color : COLORS[0];
}
function languageLabel(language: string) {
  return (
    (
      {
        python: "Python",
        typescript: "TypeScript",
        javascript: "JavaScript",
        rust: "Rust",
        json: "JSON",
        markdown: "Markdown",
        tsx: "TypeScript",
        jsx: "JavaScript",
        plaintext: "纯文本",
      } as Record<string, string>
    )[language] || language
  );
}
function monacoLanguage(language: string) {
  if (language === "c") return "cpp";
  return language === "tsx"
    ? "typescript"
    : language === "jsx"
      ? "javascript"
      : language;
}
function dateLabel(date: string | null) {
  if (!date) return "尚未确认";
  const value = /^\d+$/.test(date)
    ? new Date(Number(date) * (date.length <= 10 ? 1000 : 1))
    : new Date(date);
  return Number.isNaN(value.valueOf())
    ? date
    : value.toLocaleString("zh-CN", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      });
}
function featureAt(project: ProjectSnapshot, path: string, line: number) {
  return project.features.filter((feature) =>
    feature.anchors.some(
      (anchor) =>
        !["missing", "ambiguous"].includes(anchor.status) &&
        anchor.range.file === path &&
        anchor.range.startLine <= line &&
        anchor.range.endLine >= line,
    ),
  );
}
function statusIcon(state: ReviewState, size = 13) {
  if (state === "verified") return <CheckCircle2 size={size} />;
  if (state === "needsReview") return <TriangleAlert size={size} />;
  if (state === "reading") return <CircleDot size={size} />;
  return <Circle size={size} />;
}
function StatusBadge({ state }: { state: ReviewState }) {
  return (
    <span className={`status-badge ${REVIEW[state].className}`}>
      {statusIcon(state)}
      {REVIEW[state].text}
    </span>
  );
}

const editorOptions: editor.IStandaloneEditorConstructionOptions = {
  readOnly: true,
  domReadOnly: true,
  fontFamily: "'SFMono-Regular', Consolas, 'Liberation Mono', monospace",
  fontSize: 12.5,
  lineHeight: 22,
  minimap: { enabled: false },
  scrollBeyondLastLine: false,
  padding: { top: 20, bottom: 20 },
  renderLineHighlight: "line",
  lineNumbersMinChars: 3,
  glyphMargin: true,
  folding: true,
  automaticLayout: true,
  tabSize: 4,
  wordWrap: "off",
  smoothScrolling: true,
  contextmenu: true,
  occurrencesHighlight: "off",
  scrollbar: { verticalScrollbarSize: 7, horizontalScrollbarSize: 7 },
  overviewRulerBorder: false,
  hideCursorInOverviewRuler: true,
  guides: { indentation: true, bracketPairs: false },
  bracketPairColorization: { enabled: true },
};

function configureTheme(
  monaco: Parameters<
    NonNullable<React.ComponentProps<typeof Editor>["beforeMount"]>
  >[0],
) {
  monaco.editor.defineTheme("codetrail", {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "comment", foreground: "697889", fontStyle: "italic" },
      { token: "keyword", foreground: "c2a3f5" },
      { token: "string", foreground: "a4d3aa" },
      { token: "number", foreground: "e3bc7d" },
      { token: "type.identifier", foreground: "80ced1" },
      { token: "identifier", foreground: "d5dce9" },
    ],
    colors: {
      "editor.background": "#10151e",
      "editor.foreground": "#cdd6e5",
      "editorLineNumber.foreground": "#435065",
      "editorLineNumber.activeForeground": "#a4aec4",
      "editor.selectionBackground": "#493b6b70",
      "editor.inactiveSelectionBackground": "#34425950",
      "editor.lineHighlightBackground": "#17202c",
      "editorCursor.foreground": "#b7a0ef",
      "editorIndentGuide.background1": "#212b39",
      "editorIndentGuide.activeBackground1": "#3d4b60",
      "editorWidget.background": "#1a2331",
      "editorWidget.border": "#354158",
      "editorGutter.background": "#10151e",
      "scrollbarSlider.background": "#3b485355",
      "scrollbarSlider.hoverBackground": "#50607277",
      "diffEditor.insertedTextBackground": "#2a9c6540",
      "diffEditor.removedTextBackground": "#c65d7440",
      "diffEditor.insertedLineBackground": "#205e3820",
      "diffEditor.removedLineBackground": "#692e3e25",
    },
  });
}

export default function App() {
  const [runtimeOrigin, setRuntimeOrigin] = useState(false);
  const [runtimeContext, setRuntimeContext] = useState("运行记录");
  const [project, setProject] = useState<ProjectSnapshot | null>(null);
  const [booting, setBooting] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<{
    text: string;
    error?: boolean;
  } | null>(null);
  const [navTab, setNavTab] = useState<NavTab>("features");
  const [view, setView] = useState<View>("source");
  const [search, setSearch] = useState("");
  const [onlyChanged, setOnlyChanged] = useState(false);
  const [activeFeatureId, setActiveFeatureId] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState("");
  const [file, setFile] = useState<FileContent | null>(null);
  const [fileError, setFileError] = useState("");
  const [fileLoading, setFileLoading] = useState(false);
  const [fileDiff, setFileDiff] = useState<FileDiff | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const [diffError, setDiffError] = useState("");
  const [changes, setChanges] = useState<ChangeSummary | null>(null);
  const [changesError, setChangesError] = useState("");
  const [baseInput, setBaseInput] = useState("HEAD");
  const [headInput, setHeadInput] = useState("WORKTREE");
  const [comparison, setComparison] = useState({
    base: "HEAD",
    head: "WORKTREE",
  });
  const [splitDiff, setSplitDiff] = useState(false);
  const [cursorLine, setCursorLine] = useState(1);
  const [selection, setSelection] = useState<Selection>({
    startLine: 1,
    endLine: 1,
    empty: true,
  });
  const [jump, setJump] = useState<Jump | null>(null);
  const [relationSymbolId, setRelationSymbolId] = useState<string | null>(null);
  const [evidenceOrigin, setEvidenceOrigin] = useState<{
    symbolId: string;
    symbolName: string;
  } | null>(null);
  const [featureForm, setFeatureForm] = useState<FeatureForm | null>(null);
  const [bindForm, setBindForm] = useState<BindForm | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Feature | null>(null);
  const [report, setReport] = useState<string | null>(null);
  const [collaborationOpen,setCollaborationOpen]=useState(false);
  const [openHelp, setOpenHelp] = useState(false);
  const [inspectorVisible, setInspectorVisible] = useState(false);
  const [warningsExpanded, setWarningsExpanded] = useState(false);
  const [dataVersion, setDataVersion] = useState(0);
  const sourceEditor = useRef<editor.IStandaloneCodeEditor | null>(null);
  const sourceDecorations = useRef<editor.IEditorDecorationsCollection | null>(
    null,
  );
  const editorSubscriptions = useRef<IDisposable[]>([]);
  const revealingAnchor = useRef(false);
  const defaultReadingView = useRef(true);
  const projectRef = useRef(project);
  const selectedFileRef = useRef(selectedFile);
  const searchRef = useRef<HTMLInputElement>(null);
  projectRef.current = project;
  selectedFileRef.current = selectedFile;

  const adopt = useCallback(
    (
      snapshot: ProjectSnapshot,
      options?: { reset?: boolean; featureId?: string },
    ) => {
      setProject(snapshot);
      setActiveFeatureId(
        (current) =>
          options?.featureId ||
          (!options?.reset && snapshot.features.some((f) => f.id === current)
            ? current
            : snapshot.features[0]?.id || null),
      );
      setSelectedFile((current) =>
        !options?.reset && snapshot.files.some((f) => f.path === current)
          ? current
          : snapshot.features[0]?.anchors.find((a) => a.status !== "missing")
              ?.range.file ||
            snapshot.files[0]?.path ||
            "",
      );
      setDataVersion((version) => version + 1);
      if (options?.reset) {
        setCollaborationOpen(false);
        defaultReadingView.current = !snapshot.overview;
        setInspectorVisible(false);
        setNavTab("features");
        setView(snapshot.overview ? "overview" : "source");
        setCursorLine(1);
        setSelection({ startLine: 1, endLine: 1, empty: true });
        setRuntimeOrigin(false);
        setJump(null);
        setRelationSymbolId(null);
        setEvidenceOrigin(null);
        setSearch("");
        setOnlyChanged(false);
      }
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    api
      .loadDemo()
      .then((snapshot) => {
        if (!cancelled) adopt(snapshot, { reset: true });
      })
      .catch((error) => {
        if (!cancelled) setNotice({ text: message(error), error: true });
      })
      .finally(() => {
        if (!cancelled) setBooting(false);
      });
    return () => {
      cancelled = true;
    };
  }, [adopt]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(
      () => setNotice(null),
      notice.error ? 10000 : 4000,
    );
    return () => window.clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === "k") {
        event.preventDefault();
        searchRef.current?.focus();
      }
      if (event.key === "Escape") {
        setFeatureForm(null);
        setBindForm(null);
        setDeleteTarget(null);
        setReport(null);
        setOpenHelp(false);
      }
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);

  useEffect(
    () => () => {
      editorSubscriptions.current.forEach((subscription) =>
        subscription.dispose(),
      );
    },
    [],
  );

  useEffect(() => {
    if (!project) return;
    let cancelled = false;
    setChangesError("");
    api
      .getChanges(comparison.base, comparison.head)
      .then((result) => {
        if (!cancelled) {
          setChanges(result);
          if (defaultReadingView.current) {
            setView(result.files.some((f) => f.path === selectedFileRef.current) ? "diff" : "source");
            defaultReadingView.current = false;
          }
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setChanges(null);
          setChangesError(message(error));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [project, comparison]);

  useEffect(() => {
    if (!project || !selectedFile || view !== "source") return;
    let cancelled = false;
    setFileLoading(true);
    setFileError("");
    setFile(null);
    api
      .readFile(selectedFile)
      .then((result) => {
        if (!cancelled) {
          if (
            jump?.expectedHash &&
            jump.file === selectedFile &&
            result.hash !== jump.expectedHash
          ) {
            setFileError(
              "代码与此次导航记录的版本不一致，请返回原页面重新核对位置。",
            );
            return;
          }
          setFile(result);
        }
      })
      .catch((error) => {
        if (!cancelled) setFileError(message(error));
      })
      .finally(() => {
        if (!cancelled) setFileLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [project?.root, selectedFile, view, dataVersion, jump?.expectedHash]);

  useEffect(() => {
    if (!project || !selectedFile || view !== "diff") return;
    let cancelled = false;
    setDiffLoading(true);
    setDiffError("");
    setFileDiff(null);
    api
      .getFileDiff(selectedFile, comparison.base, comparison.head)
      .then((result) => {
        if (!cancelled) setFileDiff(result);
      })
      .catch((error) => {
        if (!cancelled) setDiffError(message(error));
      })
      .finally(() => {
        if (!cancelled) setDiffLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [project?.root, selectedFile, view, comparison, dataVersion]);

  const activeFeature = useMemo(
    () => project?.features.find((f) => f.id === activeFeatureId) || null,
    [project, activeFeatureId],
  );
  const cursorFeatures = useMemo(
    () => (project ? featureAt(project, selectedFile, cursorLine) : []),
    [project, selectedFile, cursorLine],
  );
  const currentSymbols = useMemo(
    () => project?.symbols.filter((s) => s.range.file === selectedFile) || [],
    [project, selectedFile],
  );
  const currentSymbol = useMemo(
    () =>
      currentSymbols
        .filter(
          (s) =>
            s.range.startLine <= cursorLine && s.range.endLine >= cursorLine,
        )
        .sort(
          (a, b) =>
            a.range.endLine -
            a.range.startLine -
            (b.range.endLine - b.range.startLine),
        )[0] || null,
    [currentSymbols, cursorLine],
  );
  const changedPaths = useMemo(
    () => new Set(changes?.files.map((f) => f.path) || []),
    [changes],
  );
  const changedFeatureIds = useMemo(
    () => new Set(changes?.files.flatMap((f) => f.featureIds) || []),
    [changes],
  );
  const currentChange = changes?.files.find((f) => f.path === selectedFile);
  const query = search.trim().toLocaleLowerCase();
  const filteredFeatures =
    project?.features.filter(
      (f) =>
        (!query ||
          `${f.name} ${f.description}`.toLocaleLowerCase().includes(query)) &&
        (!onlyChanged ||
          changedFeatureIds.has(f.id) ||
          f.reviewState === "needsReview"),
    ) || [];
  const filteredFiles =
    project?.files.filter(
      (f) =>
        (!query || f.path.toLocaleLowerCase().includes(query)) &&
        (!onlyChanged || changedPaths.has(f.path)),
    ) || [];
  const filteredChanges =
    changes?.files.filter(
      (f) => !query || f.path.toLocaleLowerCase().includes(query),
    ) || [];
  const verifiedCount =
    project?.features.filter((f) => f.reviewState === "verified").length || 0;
  const demo = !api.isDesktop || !!project?.root.startsWith("demo://");
  const warnings = [...(project?.warnings || []), ...(changes?.warnings || [])];

  const paintEditor = useCallback(() => {
    if (
      !sourceEditor.current?.getModel() ||
      sourceEditor.current.getModel()?.isDisposed()
    )
      return;
    const decorations: editor.IModelDeltaDecoration[] = [];
    activeFeature?.anchors
      .filter(
        (a) =>
          a.range.file === selectedFile &&
          !["missing", "ambiguous"].includes(a.status),
      )
      .forEach((anchor) => {
        decorations.push({
          range: {
            startLineNumber: anchor.range.startLine,
            startColumn: 1,
            endLineNumber: anchor.range.endLine,
            endColumn: 1,
          },
          options: {
            isWholeLine: true,
            className: "feature-anchor-line",
            linesDecorationsClassName: "feature-anchor-gutter",
            hoverMessage: {
              value: `功能：${activeFeature.name}`,
              isTrusted: false,
            },
          },
        });
      });
    // A unified diff hunk also contains unchanged context. Only mark actual
    // inserted/replacement lines here; deleted lines remain visible in Diff.
    currentChange?.hunks.forEach((hunk) => {
      hunk.lines
        .filter((line) => line.kind === "add" && line.newLine !== null)
        .forEach((line) =>
          decorations.push({
            range: {
              startLineNumber: line.newLine!,
              startColumn: 1,
              endLineNumber: line.newLine!,
              endColumn: 1,
            },
            options: {
              isWholeLine: true,
              linesDecorationsClassName: "changed-line-gutter",
            },
          }),
        );
    });
    if (
      jump?.expectedHash &&
      jump.file === selectedFile &&
      file?.hash === jump.expectedHash
    ) {
      decorations.push({
        range: {
          startLineNumber: jump.startLine,
          startColumn: jump.column || 1,
          endLineNumber: jump.endLine,
          endColumn: 1,
        },
        options: {
          isWholeLine: true,
          className: "relation-evidence-line",
          linesDecorationsClassName: "relation-evidence-gutter",
          hoverMessage: { value: "已核验的关系证据", isTrusted: false },
        },
      });
    }
    sourceDecorations.current?.set(decorations);
  }, [activeFeature, selectedFile, currentChange, jump, file]);

  const reveal = useCallback(
    (target: Jump | null) => {
      const codeEditor = sourceEditor.current;
      if (
        !codeEditor ||
        !target ||
        target.file !== selectedFile ||
        !codeEditor.getModel()
      )
        return;
      const lineCount = codeEditor.getModel()!.getLineCount();
      const start = Math.min(Math.max(1, target.startLine), lineCount);
      const end = Math.min(Math.max(start, target.endLine), lineCount);
      const column = Math.min(
        Math.max(1, target.column || 1),
        codeEditor.getModel()!.getLineMaxColumn(start),
      );
      codeEditor.revealRangeInCenter({
        startLineNumber: start,
        startColumn: column,
        endLineNumber: end,
        endColumn: 1,
      });
      // Programmatic reveal must not replace the feature explicitly chosen by
      // the reader (including a newly-created feature with no bindings yet).
      revealingAnchor.current = true;
      try {
        if (target.select) {
          codeEditor.setSelection({
            startLineNumber: start,
            startColumn: column,
            endLineNumber: end,
            endColumn: codeEditor.getModel()!.getLineMaxColumn(end),
          });
        } else codeEditor.setPosition({ lineNumber: start, column });
      } finally {
        revealingAnchor.current = false;
      }
      codeEditor.focus();
    },
    [selectedFile],
  );

  useEffect(() => {
    paintEditor();
  }, [paintEditor, file, view]);
  useEffect(() => {
    if (view === "source" && file?.path === selectedFile) {
      const frame = requestAnimationFrame(() => reveal(jump));
      return () => cancelAnimationFrame(frame);
    }
  }, [file, jump, view, selectedFile, reveal]);

  const onEditorMount: OnMount = (codeEditor) => {
    editorSubscriptions.current.forEach((subscription) =>
      subscription.dispose(),
    );
    sourceEditor.current = codeEditor;
    sourceDecorations.current = codeEditor.createDecorationsCollection();
    editorSubscriptions.current = [
      codeEditor.onDidChangeCursorPosition((event) => {
        setCursorLine(event.position.lineNumber);
        const snapshot = projectRef.current;
        if (snapshot && !revealingAnchor.current) {
          const candidates = featureAt(
            snapshot,
            selectedFileRef.current,
            event.position.lineNumber,
          );
          if (candidates.length === 1) setActiveFeatureId(candidates[0].id);
        }
      }),
      codeEditor.onDidChangeCursorSelection((event) => {
        setSelection({
          startLine: event.selection.startLineNumber,
          endLine:
            event.selection.endLineNumber > event.selection.startLineNumber &&
            event.selection.endColumn === 1
              ? event.selection.endLineNumber - 1
              : event.selection.endLineNumber,
          empty: event.selection.isEmpty(),
        });
      }),
    ];
    paintEditor();
    reveal(jump);
  };

  const onDiffMount: DiffOnMount = (codeEditor) => {
    // These anonymous diff models are owned by this widget, unlike the named
    // source models. Monaco 0.57 requires detaching them before disposal; the
    // React wrapper otherwise disposes them before disposing the widget.
    const ownedModels = codeEditor.getModel();
    codeEditor.onDidDispose(() => {
      queueMicrotask(() => {
        if (ownedModels && !ownedModels.original.isDisposed())
          ownedModels.original.dispose();
        if (ownedModels && !ownedModels.modified.isDisposed())
          ownedModels.modified.dispose();
      });
    });
    const first = currentChange?.hunks[0];
    if (first)
      codeEditor
        .getModifiedEditor()
        .revealLineInCenter(Math.max(1, first.newStart));
  };

  const run = async (label: string, action: () => Promise<void>) => {
    if (busy) return;
    setBusy(label);
    try {
      await action();
    } catch (error) {
      setNotice({ text: message(error), error: true });
    } finally {
      setBusy(null);
    }
  };

  function goToAnchor(anchor: AnchorBinding, feature?: Feature) {
    setRuntimeOrigin(false);
    if (["missing", "ambiguous"].includes(anchor.status)) {
      setNotice({
        text: "此绑定需要修复。请打开正确的代码，选中片段后点击“重新绑定”。",
        error: true,
      });
      if (feature) setActiveFeatureId(feature.id);
      return;
    }
    if (feature) setActiveFeatureId(feature.id);
    setSelectedFile(anchor.range.file);
    setView(changedPaths.has(anchor.range.file) ? "diff" : "source");
    setJump({ ...anchor.range, stamp: Date.now() });
  }

  function selectFeature(feature: Feature) {
    setActiveFeatureId(feature.id);
    const anchor = feature.anchors.find(
      (a) => !["missing", "ambiguous"].includes(a.status),
    );
    if (anchor) goToAnchor(anchor, feature);
    else setView("source");
  }

  function selectFile(path: string, diff = false) {
    setRuntimeOrigin(false);
    setSelectedFile(path);
    setJump(null);
    setCursorLine(1);
    setSelection({ startLine: 1, endLine: 1, empty: true });
    setView(diff ? "diff" : "source");
  }

  function openRuntimeCode(location: RuntimeLocation) {
    setActiveFeatureId(project?.features.find(f=>f.anchors.some(a=>a.range.file===location.path && a.range.startLine<=location.startLine && a.range.endLine>=location.startLine))?.id || null);
    setSelectedFile(location.path);setEvidenceOrigin(null);setRuntimeOrigin(true);
    setJump({file:location.path,startLine:location.startLine,endLine:location.endLine,expectedHash:location.fileHash,stamp:Date.now()});setView("source");
  }

  function openRelations() {
    const anchorSymbol = activeFeature?.anchors.find(
      (anchor) =>
        anchor.symbolId &&
        anchor.range.file === selectedFile &&
        !["missing", "ambiguous"].includes(anchor.status),
    );
    const candidate =
      currentSymbol?.id ||
      anchorSymbol?.symbolId ||
      currentSymbols[0]?.id ||
      relationSymbolId ||
      project?.symbols[0]?.id ||
      null;
    setRelationSymbolId(candidate);
    setView("relations");
  }

  function navigateRelation(navigation: RelationNavigation) {
    setRuntimeOrigin(false);
    setSelectedFile(navigation.range.file);
    setFile(navigation.content);
    setCursorLine(navigation.range.startLine);
    setJump({
      ...navigation.range,
      column: navigation.column,
      select: true,
      expectedHash: navigation.expectedHash,
      stamp: Date.now(),
    });
    setEvidenceOrigin({
      symbolId: navigation.symbolId,
      symbolName: navigation.symbolName,
    });
    setRelationSymbolId(navigation.symbolId);
    setView("source");
  }

  function newFeature() {
    setFeatureForm({
      name: "",
      description: "",
      color: COLORS[(project?.features.length || 0) % COLORS.length],
      parentId: "",
    });
  }

  function beginBinding(anchorId?: string) {
    if (!project?.features.length) {
      newFeature();
      setNotice({ text: "先创建一个功能，再为它绑定代码片段。" });
      return;
    }
    if (!file || view !== "source") {
      setNotice({ text: "请先切换到代码阅读，打开文件并选中要绑定的代码。" });
      return;
    }
    const symbol = selection.empty
      ? currentSymbol
      : currentSymbols.find(
          (s) =>
            s.range.startLine === selection.startLine &&
            s.range.endLine === selection.endLine,
        ) || null;
    const range = symbol
      ? symbol.range
      : {
          file: selectedFile,
          startLine: selection.startLine,
          endLine: selection.endLine,
        };
    setBindForm({
      featureId: activeFeatureId || project.features[0].id,
      range,
      symbol,
      note: "",
      anchorId,
    });
  }

  async function submitFeature(event: React.FormEvent) {
    event.preventDefault();
    if (!featureForm) return;
    const draft: FeatureDraft = {
      id: featureForm.id,
      name: featureForm.name.trim(),
      description: featureForm.description.trim(),
      color: featureForm.color,
      parentId: featureForm.parentId || null,
    };
    if (!draft.name) return;
    await run("保存功能", async () => {
      const snapshot = await api.saveFeature(draft);
      const saved =
        snapshot.features.find((f) => f.id === draft.id) ||
        snapshot.features.find((f) => f.name === draft.name);
      adopt(snapshot, { featureId: saved?.id });
      setFeatureForm(null);
      setNavTab("features");
      setNotice({
        text: draft.id
          ? "功能信息已更新。"
          : "功能已创建，选中代码即可添加第一个片段。",
      });
    });
  }

  async function submitBinding(event: React.FormEvent) {
    event.preventDefault();
    if (!bindForm) return;
    await run("绑定代码", async () => {
      const snapshot = await api.bindAnchor({
        featureId: bindForm.featureId,
        path: bindForm.range.file,
        startLine: bindForm.range.startLine,
        endLine: bindForm.range.endLine,
        symbolId: bindForm.symbol?.id || null,
        note: bindForm.note.trim(),
        anchorId: bindForm.anchorId || null,
      });
      adopt(snapshot, { featureId: bindForm.featureId });
      setBindForm(null);
      setNotice({
        text: bindForm.anchorId
          ? "代码绑定已修复，请重新确认功能。"
          : "代码片段已绑定，功能与代码现在可以双向定位。",
      });
    });
  }

  async function openProject() {
    if (!api.isDesktop) {
      setOpenHelp(true);
      return;
    }
    await run("打开仓库", async () => {
      const snapshot = await api.chooseProject();
      if (snapshot) {
        adopt(snapshot, { reset: true });
        setComparison({ base: "HEAD", head: "WORKTREE" });
        setBaseInput("HEAD");
        setHeadInput("WORKTREE");
      }
    });
  }

  async function updateReview(state: ReviewState) {
    if (!activeFeature) return;
    await run("更新审阅状态", async () => {
      adopt(
        await api.setReview(
          activeFeature.id,
          state,
          activeFeature.currentDigest,
        ),
      );
      setNotice({
        text:
          state === "verified"
            ? "已确认当前代码版本；刷新时发现内容变化，会提示复核。"
            : "审阅状态已更新。",
      });
    });
  }

  async function exportReport() {
    await run("生成阅读记录", async () => setReport(await api.exportReview()));
  }

  function downloadReport() {
    if (report === null) return;
    const url = URL.createObjectURL(
      new Blob([report], { type: "text/markdown;charset=utf-8" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = `${project?.name || "project"}-review.md`;
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  if (booting)
    return (
      <div className="boot-screen">
        <div className="brand-mark">
          <Layers size={25} />
        </div>
        <h1>CodeTrail</h1>
        <p>正在准备你的代码阅读工作台</p>
        <Loader2 className="spin" size={22} />
      </div>
    );

  return (
    <div className={`app-shell ${inspectorVisible ? "" : "inspector-hidden"}`}>
      <header className="app-header">
        <div className="brand">
          <div className="brand-mark">
            <Layers size={21} strokeWidth={1.8} />
          </div>
          <div>
            <strong>
              CodeTrail<span className="brand-dot">.</span>
            </strong>
            <span className="brand-subtitle">代码阅读器</span>
          </div>
        </div>
        <div className="header-divider" />
        <div className="header-workspace">
          <BookOpen size={15} />
          <span>阅读工作台</span>
        </div>
        <div className="header-spacer" />
        <span className="local-indicator">
          <span /> 本地运行 · 源码只读
        </span>
        <button
          className="icon-button"
          onClick={() =>
            run("刷新索引", async () => {
              adopt(await api.refresh());
              setNotice({ text: "索引已刷新，已检查绑定和审阅状态。" });
            })
          }
          disabled={!!busy || !project}
          title="刷新代码与索引"
          aria-label="刷新代码与索引"
        >
          <RefreshCw size={16} className={busy === "刷新索引" ? "spin" : ""} />
        </button>
        <button className="button" disabled={!project||!!busy} onClick={()=>{setInspectorVisible(false);setCollaborationOpen(true);}}>共读</button>
        <button
          className="button button-primary open-project"
          onClick={openProject}
          disabled={!!busy}
        >
          <FolderOpen size={15} />
          打开仓库
        </button>
      </header>

      <section className="project-bar">
        <div className="project-identity">
          <div className="project-icon">
            <Code2 size={23} />
          </div>
          <div>
            <div className="project-title">
              <h1>{project?.name || "还没有打开项目"}</h1>
              {demo && <span className="demo-tag">演示项目</span>}
              <span className="version-tag">v0.3.1 Preview</span>
            </div>
            <div className="project-meta">
              <GitBranch size={12} />
              <span>
                {project?.git.branch ||
                  (project?.git.available ? "未提交" : "无 Git 仓库")}
              </span>
              <span className="meta-separator">/</span>
              <span title={project?.root}>
                {demo
                  ? "体验功能绑定与改动审阅，标注仅保存在本机"
                  : project?.root}
              </span>
            </div>
          </div>
        </div>
        <span className="reading-step">选功能 <ChevronRight size={12} /> 读代码和改动 <ChevronRight size={12} /> 确认已读</span>
      </section>

      <div className="workspace">
        <aside className="sidebar">
          {project && <button className={`overview-nav ${view === "runs" ? "active" : ""}`} onClick={() => {setView("runs");setInspectorVisible(false);}}>运行记录</button>}
          {project?.overview && <button className={`overview-nav ${view === "overview" ? "active" : ""}`} onClick={() => { setView("overview"); setInspectorVisible(false); }}>项目概览</button>}
          <div className="simple-sidebar-heading">选功能</div>
          <details className="secondary-navigation"><summary>浏览文件与改动</summary>          <div className="navigator-tabs" role="tablist" aria-label="项目导航">
            <button
              role="tab"
              aria-selected={navTab === "features"}
              onClick={() => setNavTab("features")}
              className={navTab === "features" ? "active" : ""}
            >
              <Map size={14} />
              功能
            </button>
            <button
              role="tab"
              aria-selected={navTab === "files"}
              onClick={() => setNavTab("files")}
              className={navTab === "files" ? "active" : ""}
            >
              <FileCode2 size={14} />
              代码
            </button>
            <button
              role="tab"
              aria-selected={navTab === "changes"}
              onClick={() => setNavTab("changes")}
              className={navTab === "changes" ? "active" : ""}
            >
              <GitCompareArrows size={14} />
              改动
              {changes?.files.length ? (
                <span className="tab-count">{changes.files.length}</span>
              ) : null}
            </button>
          </div>
          </details>
          <div className="sidebar-controls">
            <div className="search-input">
              <Search size={14} />
              <input
                ref={searchRef}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="搜索函数、变量、功能或文件…"
                maxLength={200}
                aria-label="搜索函数、变量、功能或文件"
              />
              {search ? (
                <button aria-label="清除搜索" onClick={() => setSearch("")}>
                  <X size={12} />
                </button>
              ) : (
                <kbd>⌘ K</kbd>
              )}
            </div>
            <details className="secondary-filter"><summary>筛选</summary>            <label className="filter-toggle">
              <input
                type="checkbox"
                checked={onlyChanged}
                onChange={(e) => setOnlyChanged(e.target.checked)}
              />
              <span className="toggle-track" />
              <span>只看改动相关</span>
              <span className="filter-dot" />
            </label></details>
          </div>

          <div className="sidebar-section-label">
            <span>
              {navTab === "features"
                ? "功能索引"
                : navTab === "files"
                  ? "仓库文件"
                  : "变更清单"}
            </span>
            <span>
              {navTab === "features"
                ? filteredFeatures.length
                : navTab === "files"
                  ? filteredFiles.length
                  : filteredChanges.length}
            </span>
          </div>
          <div className="navigation-list">
            {project && query && <CodeSearch key={project.root} project={project} query={search} revision={dataVersion} changedPaths={onlyChanged ? changedPaths : undefined} onNavigate={match => {
              setRuntimeOrigin(false); setEvidenceOrigin(null); setSelectedFile(match.path);
              setActiveFeatureId(project.features.find(f => f.anchors.some(a => a.range.file === match.path && a.range.startLine <= match.line && a.range.endLine >= match.line))?.id || null);
              setJump({ file: match.path, startLine: match.line, endLine: match.line, expectedHash: match.fileHash || undefined, stamp: Date.now() });
              setCursorLine(match.line); setView("source");
            }} />}

            {navTab === "features" &&
              filteredFeatures.map((feature) => (
                <button
                  key={feature.id}
                  onClick={() => selectFeature(feature)}
                  className={`feature-nav-item ${view !== "overview" && view !== "runs" && activeFeatureId === feature.id ? "selected" : ""} ${cursorFeatures.some((f) => f.id === feature.id) ? "cursor-match" : ""}`}
                  aria-label={`查看功能：${feature.name}`}
                >
                  <span
                    className="feature-nav-icon"
                    style={{
                      color: safeColor(feature.color),
                      backgroundColor: `${safeColor(feature.color)}14`,
                    }}
                  >
                    <Layers size={15} />
                  </span>
                  <span className="feature-nav-content">
                    <span className="feature-nav-name">{feature.name}</span>
                    <span className="feature-nav-detail">
                      {feature.anchors.length} 个片段<span>·</span>
                      <span className={REVIEW[feature.reviewState].className}>
                        {REVIEW[feature.reviewState].text}
                      </span>
                    </span>
                  </span>
                  {feature.reviewState === "needsReview" ? (
                    <span className="needs-review-dot" />
                  ) : (
                    <ChevronRight size={13} className="nav-chevron" />
                  )}
                </button>
              ))}
            {navTab === "features" && !filteredFeatures.length && (
              <SmallEmpty
                icon={<Layers size={22} />}
                title={
                  search || onlyChanged ? "没有匹配的功能" : "从一个功能开始"
                }
                text={
                  search || onlyChanged
                    ? "试试其他关键词或关闭筛选。"
                    : "将分散的代码片段组织成你理解的功能。"
                }
              />
            )}
            {navTab === "files" &&
              filteredFiles.map((entry) => (
                <button
                  key={entry.path}
                  className={`file-nav-item ${entry.path === selectedFile ? "selected" : ""}`}
                  onClick={() => selectFile(entry.path)}
                  title={entry.path}
                >
                  <FileCode2
                    size={16}
                    className={`file-icon lang-${entry.language}`}
                  />
                  <span>
                    <strong>{filename(entry.path)}</strong>
                    <small>{dirname(entry.path)}</small>
                  </span>
                  {changedPaths.has(entry.path) && (
                    <span className="file-modified">M</span>
                  )}
                </button>
              ))}
            {navTab === "files" && !filteredFiles.length && (
              <SmallEmpty
                icon={<Search size={22} />}
                title="没有匹配的文件"
                text="可以搜索完整路径或文件名。"
              />
            )}
            {navTab === "changes" && changesError && (
              <div className="inline-error">
                <TriangleAlert size={17} />
                <p>{changesError}</p>
              </div>
            )}
            {navTab === "changes" &&
              filteredChanges.map((change) => (
                <button
                  key={change.path}
                  className={`change-nav-item ${change.path === selectedFile && view === "diff" ? "selected" : ""}`}
                  onClick={() => selectFile(change.path, true)}
                  title={change.path}
                >
                  <span className={`change-status-letter ${change.status}`}>
                    {change.status === "deleted"
                      ? "D"
                      : change.status === "added" ||
                          change.status === "untracked"
                        ? "A"
                        : change.status === "renamed"
                          ? "R"
                          : "M"}
                  </span>
                  <span className="change-nav-content">
                    <strong>{filename(change.path)}</strong>
                    <small>{dirname(change.path)}</small>
                    <span className="change-nav-counts">
                      <span className="added">+{change.additions}</span>
                      <span className="removed">−{change.deletions}</span>
                      <span>{CHANGE_STATUS[change.status]}</span>
                    </span>
                  </span>
                  <ChevronRight size={13} />
                </button>
              ))}
            {navTab === "changes" &&
              !filteredChanges.length &&
              !changesError && (
                <SmallEmpty
                  icon={<CheckCircle2 size={23} />}
                  title="没有匹配的改动"
                  text={
                    project?.git.available
                      ? "当前比较范围内没有文件变化。"
                      : "文件和功能阅读仍可正常使用。"
                  }
                />
              )}
          </div>
          <div className="sidebar-footer">
            <div className="reading-progress">
              <span>
                已读 {verifiedCount} / {project?.features.length || 0}
              </span>
              <span>
                {project?.features.length
                  ? Math.round((100 * verifiedCount) / project.features.length)
                  : 0}
                %
              </span>
            </div>
            <div className="progress-track">
              <span
                style={{
                  width: `${project?.features.length ? (100 * verifiedCount) / project.features.length : 0}%`,
                }}
              />
            </div>
          </div>
        </aside>

        <main className="main-pane">
          {activeFeature && view !== "overview" && view !== "runs" && (
            <section className="reading-intro" aria-label="当前功能">
              <div className="reading-title-row">
                <div><h2>{activeFeature.name}</h2><StatusBadge state={activeFeature.reviewState} /></div>
                <button className="button button-verified" onClick={() => updateReview("verified")}
                  disabled={!!busy || !activeFeature.anchors.length || activeFeature.anchors.some(a => ["missing", "ambiguous"].includes(a.status)) || activeFeature.reviewState === "verified"}>
                  <Check size={15} />{activeFeature.reviewState === "verified" ? "已读当前版本" : "确认已读"}
                </button>
              </div>
              <p>{activeFeature.description || "选择下方代码片段，开始阅读。"}</p>
              {activeFeature.reviewState === "needsReview" && <p className="reading-change-note">代码或说明已变化，请重新阅读后确认。</p>}
              {!!activeFeature.anchors.length && <div className="reading-evidence">
                <label>阅读片段
                  <select aria-label="阅读片段" value={activeFeature.anchors.find(a => a.range.file === selectedFile && a.range.startLine === jump?.startLine)?.id || activeFeature.anchors.find(a => a.range.file === selectedFile)?.id || ""}
                    onChange={event => { const a = activeFeature.anchors.find(a => a.id === event.target.value); if (a) goToAnchor(a, activeFeature); }}>
                    <option value="" disabled>选择片段</option>
                    {activeFeature.anchors.map((a, i) => <option key={a.id} value={a.id}>{i + 1}. {a.symbolName || filename(a.range.file)} · {a.responsibility || a.note || ANCHOR_STATUS[a.status]}</option>)}
                  </select>
                </label>
                <span>{(activeFeature.anchors.find(a => a.range.file === selectedFile && a.range.startLine === jump?.startLine) || activeFeature.anchors.find(a => a.range.file === selectedFile))?.responsibility}</span>
              </div>}
            </section>
          )}
          {view !== "overview" && view !== "runs" && <div className="view-toolbar">
            <div className="view-tabs" role="tablist" aria-label="阅读视图">
              <button role="tab" aria-selected={view === "diff"} className={view === "diff" ? "active" : ""}
                disabled={!project?.git.available && !demo} onClick={() => setView("diff")}><GitCompareArrows size={14} />本次改动</button>
              <button role="tab" aria-selected={view === "source"} className={view === "source" ? "active" : ""}
                onClick={() => setView("source")}><Code2 size={14} />完整代码</button>
            </div>
            <details className="reading-tools"><summary><MoreHorizontal size={16} />更多</summary>
              <div className="reading-tools-menu" onClick={event => { const details = event.currentTarget.closest("details"); if (details) details.open = false; }}>
                <button onClick={() => setInspectorVisible(v => !v)}>{inspectorVisible ? "收起详情" : "查看详情"}</button>
                <button onClick={openRelations}>相关逻辑</button>
                <button onClick={newFeature} disabled={!project || !!busy}>新建功能</button>
                {!activeFeature?.agentDelivery && <button onClick={() => beginBinding()} disabled={!file || !!busy}>绑定当前片段</button>}
                <button onClick={exportReport} disabled={!project || !!busy}>导出阅读记录</button>
              </div>
            </details>
          </div>}

          {project && <div className="runtime-host" hidden={view !== "runs"}><RuntimePane key={project.root} root={project.root} demo={demo} onCode={openRuntimeCode} onContext={setRuntimeContext}/></div>}
          {view === "runs" ? null : view === "overview" && project ? (
            <ProjectOverview project={project} onRead={goToAnchor} />
          ) : view === "relations" && project ? (
            <RelationsPane
              project={project}
              symbolId={relationSymbolId}
              base={comparison.base}
              head={comparison.head}
              revision={dataVersion}
              onSelectSymbol={setRelationSymbolId}
              onNavigate={navigateRelation}
              onSelectFeature={setActiveFeatureId}
              onUseWorktree={() => {
                setHeadInput("WORKTREE");
                setComparison((current) => ({ ...current, head: "WORKTREE" }));
              }}
            />
          ) : (
            <>
              {view === "diff" && (
                <details className="comparison-options"><summary>对比设置</summary>                <form
                  className="comparison-bar"
                  onSubmit={(event) => {
                    event.preventDefault();
                    setComparison({
                      base: baseInput.trim() || "HEAD",
                      head: headInput.trim() || "WORKTREE",
                    });
                  }}
                >
                  <GitBranch size={13} />
                  <label>
                    <span>基线</span>
                    <input
                      aria-label="对比基线"
                      value={baseInput}
                      onChange={(e) => setBaseInput(e.target.value)}
                      spellCheck={false}
                    />
                  </label>
                  <ArrowUpRight size={14} />
                  <label>
                    <span>目标</span>
                    <input
                      aria-label="对比目标"
                      value={headInput}
                      onChange={(e) => setHeadInput(e.target.value)}
                      spellCheck={false}
                    />
                  </label>
                  <button type="submit" className="button button-compact">
                    比较
                  </button>
                  <button
                    type="button"
                    className={`icon-button diff-layout ${splitDiff ? "is-on" : ""}`}
                    onClick={() => setSplitDiff((value) => !value)}
                    title={splitDiff ? "切换统一对比" : "切换并排对比"}
                    aria-label={splitDiff ? "切换统一对比" : "切换并排对比"}
                  >
                    <SplitSquareHorizontal size={15} />
                  </button>
                </form></details>
              )}
              <div className="file-tab-bar">
                <div className="active-file-tab">
                  <FileCode2 size={14} />
                  <span>{filename(selectedFile) || "选择一个文件"}</span>
                  {currentChange && (
                    <span className="tab-modified">
                      {CHANGE_STATUS[currentChange.status]}
                    </span>
                  )}
                </div>
                {view === "diff" && currentChange && (
                  <div className="diff-totals">
                    <span className="added">+{currentChange.additions}</span>
                    <span className="removed">−{currentChange.deletions}</span>
                  </div>
                )}
              </div>
              <div className="breadcrumb">
                <span>{project?.name}</span>
                <ChevronRight size={11} />
                <span className="breadcrumb-path">
                  {selectedFile || "未选择文件"}
                </span>
                {view === "source" && currentSymbol && (
                  <>
                    <ChevronRight size={11} />
                    <span className="breadcrumb-symbol">
                      {currentSymbol.name}
                    </span>
                  </>
                )}
                <span className="read-only-label">
                  <ShieldCheck size={11} />
                  只读
                </span>
              </div>
              {view === "source" && runtimeOrigin && <div className="relation-source-banner"><span>来自本次运行记录</span><button onClick={()=>setView("runs")}>返回本次运行</button></div>}
              {view === "source" && jump?.expectedHash && evidenceOrigin && (
                <div className="relation-source-banner">
                  <Network size={13} />
                  <span>
                    来自 <strong>{evidenceOrigin.symbolName}</strong> 的关系证据
                  </span>
                  <button
                    onClick={() => {
                      setRelationSymbolId(evidenceOrigin.symbolId);
                      setView("relations");
                    }}
                  >
                    返回相关逻辑
                    <ChevronRight size={12} />
                  </button>
                </div>
              )}
              <div className="editor-container">
                {view === "source" &&
                  (fileLoading ? (
                    <EditorLoading />
                  ) : fileError ? (
                    <EditorError
                      text={fileError}
                      onRetry={() => setDataVersion((v) => v + 1)}
                    />
                  ) : file ? (
                    <Editor
                      key={`${project?.root}:source`}
                      path={`file:///codetrail/${encodeURIComponent(project?.root || "")}/${selectedFile}`}
                      value={file.content}
                      language={monacoLanguage(file.language)}
                      theme="codetrail"
                      beforeMount={configureTheme}
                      onMount={onEditorMount}
                      options={editorOptions}
                      loading={<EditorLoading />}
                    />
                  ) : (
                    <SmallEmpty
                      icon={<BookOpen size={32} />}
                      title="选择一段代码，开始理解"
                      text="从左侧功能索引或仓库文件打开源码。"
                    />
                  ))}
                {view === "diff" &&
                  (diffLoading ? (
                    <EditorLoading />
                  ) : diffError || changesError ? (
                    <EditorError
                      text={diffError || changesError}
                      onRetry={() => setDataVersion((v) => v + 1)}
                    />
                  ) : fileDiff ? (
                    <DiffEditor
                      key={`${project?.root}:${selectedFile}:${comparison.base}:${comparison.head}`}
                      original={fileDiff.oldContent}
                      modified={fileDiff.newContent}
                      keepCurrentOriginalModel
                      keepCurrentModifiedModel
                      language={monacoLanguage(fileDiff.language)}
                      theme="codetrail"
                      beforeMount={configureTheme}
                      onMount={onDiffMount}
                      options={{
                        ...editorOptions,
                        originalEditable: false,
                        renderSideBySide: splitDiff,
                        useInlineViewWhenSpaceIsLimited: false,
                        enableSplitViewResizing: true,
                        renderOverviewRuler: false,
                        renderIndicators: true,
                        diffWordWrap: "off",
                      }}
                      loading={<EditorLoading />}
                    />
                  ) : (
                    <SmallEmpty
                      icon={<GitCompareArrows size={32} />}
                      title="选择一个改动文件"
                      text="查看基线与当前代码的逐行差异。"
                    />
                  ))}
              </div>
              <div className="editor-footer">
                {view === "source" ? (
                  <>
                    <span>
                      <CircleDot size={11} />
                      {selection.empty
                        ? `Ln ${cursorLine}`
                        : `已选 L${selection.startLine}–L${selection.endLine}`}
                    </span>
                    <span>{file ? languageLabel(file.language) : "—"}</span>
                    <span>UTF-8</span>
                    <span className="editor-footer-right">
                      {cursorFeatures.length
                        ? `${cursorFeatures.length} 个关联功能`
                        : "当前行未归类"}
                    </span>
                  </>
                ) : (
                  <>
                    <span>
                      <GitCompareArrows size={12} />
                      {comparison.base} →{" "}
                      {comparison.head === "WORKTREE"
                        ? "当前工作区"
                        : comparison.head}
                    </span>
                    <span className="editor-footer-right">
                      {splitDiff ? "并排对比" : "统一对比"} · 双侧只读
                    </span>
                  </>
                )}
              </div>
            </>
          )}
        </main>

        {project && !inspectorVisible && <WorkPanel key={project.root} root={project.root} demo={demo} collaborationOpen={collaborationOpen} onCloseCollaboration={()=>setCollaborationOpen(false)} onSharedCode={location=>{setRuntimeOrigin(false);setEvidenceOrigin(null);setSelectedFile(location.path);setActiveFeatureId(project.features.find(f=>f.anchors.some(a=>a.range.file===location.path&&a.range.startLine<=location.startLine&&a.range.endLine>=location.startLine))?.id||null);setJump({file:location.path,startLine:location.startLine,endLine:location.endLine,expectedHash:location.fileHash,stamp:Date.now()});setView("source");}} context={{
          context: view === "runs" ? runtimeContext : view === "overview" ? "项目概览" : view === "relations" ? "相关逻辑" : activeFeature?.name || "代码阅读",
          featureId: view === "overview" || view === "runs" ? null : activeFeature?.id || null,
          featureName: view === "overview" || view === "runs" ? null : activeFeature?.name || null,
          path: view !== "source" || file?.path !== selectedFile ? null : file.path,
          startLine: view !== "source" || !file || file.path !== selectedFile ? null : selection.empty ? cursorLine : selection.startLine,
          endLine: view !== "source" || !file || file.path !== selectedFile ? null : selection.empty ? cursorLine : selection.endLine,
          fileHash: view !== "source" || file?.path !== selectedFile ? null : file.hash,
          quote: view !== "source" || !file || file.path !== selectedFile || selection.empty ? "" : file.content.split("\n").slice(selection.startLine-1,selection.endLine).join("\n").slice(0,4000),
        }}/>}
        {inspectorVisible && (
          <aside className="inspector">
            <div className="inspector-heading">
              <span>
                <Layers size={14} />
                功能详情
              </span>
              <button
                className="icon-button"
                title="导出阅读记录"
                aria-label="导出阅读记录"
                onClick={exportReport}
                disabled={!project || !!busy}
              >
                <Download size={15} />
              </button>
            </div>
            <div className="inspector-scroll">
              {activeFeature ? (
                <>
                  <div className="feature-overview">
                    <div className="feature-overview-top">
                      <span
                        className="feature-overview-icon"
                        style={{
                          color: safeColor(activeFeature.color),
                          background: `${safeColor(activeFeature.color)}16`,
                        }}
                      >
                        <Layers size={20} />
                      </span>
                      {!activeFeature.agentDelivery && <div className="overview-actions">
                        <button
                          className="icon-button"
                          title="编辑功能"
                          aria-label="编辑功能"
                          disabled={!!activeFeature.agentDelivery}
                          onClick={() =>
                            setFeatureForm({
                              id: activeFeature.id,
                              name: activeFeature.name,
                              description: activeFeature.description,
                              color: activeFeature.color,
                              parentId: activeFeature.parentId || "",
                            })
                          }
                        >
                          <Pencil size={13} />
                        </button>
                        <button
                          className="icon-button danger-hover"
                          title="删除功能索引"
                          aria-label="删除功能索引"
                          disabled={!!activeFeature.agentDelivery}
                          onClick={() => setDeleteTarget(activeFeature)}
                        >
                          <Trash2 size={13} />
                        </button>
                      </div>}
                    </div>
                    <h2>{activeFeature.name}</h2>
                    <details className="delivery-details"><summary>关联信息</summary><div className="feature-stable-id">功能 ID：{activeFeature.id}</div></details>
                    <StatusBadge state={activeFeature.reviewState} />
                    <p className="feature-description">
                      {activeFeature.description ||
                        "还没有功能说明。点击编辑，写下这个功能的意图和需要关注的行为。"}
                    </p>
                  </div>
                  {activeFeature.agentDelivery && (
                    <details className="inspector-section agent-delivery"><summary>交付详情</summary>
                      <h3>Agent 交付 · {activeFeature.agentDelivery.deliveryId}</h3>
                      <p>{activeFeature.agentDelivery.summary}</p>
                      <strong>{activeFeature.agentDelivery.matchesSource ? "交付证据与当前源码一致" : "交付证据已变化，请 Agent 更新"}</strong>
                      <p>人工审阅：{activeFeature.reviewState === "verified" ? "已确认当前证据" : activeFeature.reviewState === "needsReview" ? "待重新复核" : "尚未确认"}。交付校验不代表测试通过或人工确认。</p>
                      <small>关联由 Agent CLI 维护，人工阅读确认独立保存。</small>
                    </details>
                  )}
                  {activeFeature.reviewState === "needsReview" && (
                    <div className="review-alert">
                      <TriangleAlert size={16} />
                      <div>
                        <strong>已有理解需要重新检查</strong>
                        <p>
                          绑定的代码或功能内容已变化。确认当前实现后，再更新审阅状态。
                        </p>
                      </div>
                    </div>
                  )}
                  <section className="inspector-section">
                    <div className="section-heading">
                      <h3>
                        代码证据<span>{activeFeature.anchors.length}</span>
                      </h3>
                      {!activeFeature.agentDelivery &&                       <button
                        className="icon-button"
                        title="添加代码片段"
                        aria-label="添加代码片段"
                        disabled={!!activeFeature.agentDelivery}
                        onClick={() => beginBinding()}
                      >
                        <Plus size={14} />
                      </button>}
                    </div>
                    {activeFeature.anchors.length ? (
                      <div className="anchor-list">
                        {activeFeature.anchors.map((anchor, index) => (
                          <div
                            key={anchor.id}
                            className={`anchor-card ${["missing", "ambiguous"].includes(anchor.status) ? "anchor-stale" : ""}`}
                          >
                            <button
                              className="anchor-main"
                              onClick={() => goToAnchor(anchor, activeFeature)}
                              title={`${anchor.range.file}:${anchor.range.startLine}`}
                            >
                              <div className="anchor-index">
                                {String(index + 1).padStart(2, "0")}
                              </div>
                              <div>
                                <strong>
                                  {anchor.symbolName ||
                                    filename(anchor.range.file)}
                                </strong>
                                <span className="anchor-path">
                                  {anchor.range.file}
                                </span>
                                <span className="anchor-range">
                                  L{anchor.range.startLine}–
                                  {anchor.range.endLine}
                                  <span
                                    className={`anchor-health health-${anchor.status}`}
                                  >
                                    {ANCHOR_STATUS[anchor.status]}
                                  </span>
                                </span>
                              </div>
                              <ArrowUpRight size={12} />
                            </button>
                            {anchor.note && (
                              <p className="anchor-note">{anchor.note}</p>
                            )}
                            {anchor.role && <p className="anchor-responsibility"><strong>{{entry:"入口",implementation:"实现",constraint:"约束",errorHandling:"异常处理",test:"测试"}[anchor.role]}</strong> · {anchor.responsibility}</p>}
                            {!activeFeature.agentDelivery && <div className="anchor-actions">
                              <span>
                                {anchor.kind === "symbol"
                                  ? "符号绑定"
                                  : "片段绑定"}
                              </span>
                              <button
                                onClick={() => beginBinding(anchor.id)}
                                title="用当前代码选择重新绑定"
                                aria-label={`重新绑定 ${anchor.symbolName || filename(anchor.range.file)}`}
                                disabled={!!activeFeature.agentDelivery}
                              >
                                <RefreshCw size={11} />
                                重绑
                              </button>
                              <button
                                title="解除此片段绑定"
                                aria-label={`解除绑定 ${anchor.symbolName || filename(anchor.range.file)}`}
                                disabled={!!activeFeature.agentDelivery}
                                onClick={() =>
                                  run("解除绑定", async () =>
                                    adopt(
                                      await api.removeAnchor(
                                        activeFeature.id,
                                        anchor.id,
                                      ),
                                    ),
                                  )
                                }
                              >
                                <Unlink size={11} />
                              </button>
                            </div>}
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="empty-anchor">
                        <Link2 size={22} />
                        <p>选中代码，建立第一个绑定</p>
                        <span>支持整个函数，也支持函数中的几行。</span>
                        <button
                          className="text-button"
                          onClick={() => beginBinding()}
                        >
                          绑定当前片段
                          <ArrowUpRight size={12} />
                        </button>
                      </div>
                    )}
                  </section>

                  <section className="inspector-section review-section">
                    <button className="button button-subtle" onClick={() => updateReview("reading")} disabled={!!busy}>标为阅读中</button>
                    <div className="last-reviewed"><span>最近确认</span><span>{dateLabel(activeFeature.reviewedAt)}</span></div>
                  </section>
                </>
              ) : (
                <SmallEmpty
                  icon={<Layers size={28} />}
                  title="选择一个功能"
                  text="把散落的代码组织起来，保存你的理解与阅读进度。"
                  action={
                    <button
                      className="button button-subtle"
                      onClick={newFeature}
                    >
                      <Plus size={14} />
                      新建功能
                    </button>
                  }
                />
              )}

              <section className="inspector-section cursor-section">
                <div className="section-heading">
                  <h3>当前代码属于</h3>
                  <span className="line-label">L{cursorLine}</span>
                </div>
                {cursorFeatures.length ? (
                  <div className="cursor-features">
                    {cursorFeatures.map((feature) => (
                      <button
                        key={feature.id}
                        onClick={() => setActiveFeatureId(feature.id)}
                        className={
                          feature.id === activeFeatureId ? "active" : ""
                        }
                      >
                        <span
                          style={{ background: safeColor(feature.color) }}
                        />
                        {feature.name}
                        <ChevronRight size={12} />
                      </button>
                    ))}
                  </div>
                ) : (
                  <p className="section-description">
                    当前行尚未绑定功能。可以保留未归类，或将它加入已有功能。
                  </p>
                )}
              </section>
              {!!currentSymbols.length && view === "source" && (
                <section className="inspector-section symbol-section">
                  <div className="section-heading">
                    <h3>文件结构</h3>
                    <span>{currentSymbols.length}</span>
                  </div>
                  <div className="symbol-list">
                    {currentSymbols.map((symbol) => (
                      <button
                        key={symbol.id}
                        className={
                          currentSymbol?.id === symbol.id ? "current" : ""
                        }
                        onClick={() => {
                          const candidates = project
                            ? featureAt(
                                project,
                                symbol.range.file,
                                symbol.range.startLine,
                              )
                            : [];
                          if (candidates.length === 1)
                            setActiveFeatureId(candidates[0].id);
                          setJump({ ...symbol.range, stamp: Date.now() });
                        }}
                        title={symbol.signature}
                      >
                        <span className="symbol-kind">
                          {symbol.kind.includes("class") ||
                          symbol.kind.includes("struct")
                            ? "C"
                            : "ƒ"}
                        </span>
                        <span>{symbol.name}</span>
                        <small>{symbol.range.startLine}</small>
                      </button>
                    ))}
                  </div>
                </section>
              )}
            </div>
            <div className="inspector-bottom">
              <ShieldCheck size={13} />
              <span>只修改阅读标注，绝不修改源码</span>
            </div>
          </aside>
        )}
      </div>

      {warningsExpanded && warnings.length > 0 && (
        <div className="warnings-panel">
          <div>
            <strong>索引与分析提示</strong>
            <button
              className="icon-button"
              aria-label="关闭提示面板"
              onClick={() => setWarningsExpanded(false)}
            >
              <X size={15} />
            </button>
          </div>
          <ul>
            {warnings.map((warning, index) => (
              <li key={`${index}-${warning}`}>{warning}</li>
            ))}
          </ul>
        </div>
      )}
      <footer className="status-bar">
        <span className="status-project">
          <span className="status-live-dot" />
          {busy || "索引已就绪"}
        </span>
        <span>{project?.files.length || 0} 个文件</span>
        <span>{project?.symbols.length || 0} 个符号</span>
        {warnings.length > 0 && (
          <button onClick={() => setWarningsExpanded((value) => !value)}>
            <TriangleAlert size={11} />
            {warnings.length} 条提示
          </button>
        )}
        <span className="status-spacer" />
        <button
          onClick={() =>
            run("载入演示", async () => {
              adopt(await api.loadDemo(), { reset: true });
              setView("source");
              setNotice({ text: "已打开内置演示项目。" });
            })
          }
          disabled={!!busy}
        >
          {demo ? "内置演示" : "打开演示"}
          <ArrowUpRight size={10} />
        </button>
        <span className="status-mode">
          {api.isDesktop ? "Rust + Tauri" : "浏览器演示"}
          <span className="status-separator">·</span>本地优先
        </span>
      </footer>

      {notice && (
        <div
          className={`toast ${notice.error ? "toast-error" : ""}`}
          role={notice.error ? "alert" : "status"}
        >
          {notice.error ? (
            <TriangleAlert size={16} />
          ) : (
            <CheckCircle2 size={16} />
          )}
          <span>{notice.text}</span>
          <button aria-label="关闭通知" onClick={() => setNotice(null)}>
            <X size={14} />
          </button>
        </div>
      )}

      {featureForm && (
        <Modal
          title={featureForm.id ? "编辑功能" : "新建功能"}
          subtitle="用你的语言描述功能，并为它关联真实代码。"
          onClose={() => setFeatureForm(null)}
        >
          <form onSubmit={submitFeature}>
            <label className="form-label">
              功能名称
              <input
                autoFocus
                required
                maxLength={120}
                placeholder="例如：模型调用与失败降级"
                value={featureForm.name}
                onChange={(e) =>
                  setFeatureForm({ ...featureForm, name: e.target.value })
                }
              />
            </label>
            <label className="form-label">
              功能说明
              <textarea
                rows={4}
                placeholder="它解决什么问题？阅读时应该关注哪些行为？"
                value={featureForm.description}
                onChange={(e) =>
                  setFeatureForm({
                    ...featureForm,
                    description: e.target.value,
                  })
                }
              />
            </label>
            <div className="form-row">
              <label className="form-label">
                上级功能
                <select
                  value={featureForm.parentId}
                  onChange={(e) =>
                    setFeatureForm({ ...featureForm, parentId: e.target.value })
                  }
                >
                  <option value="">顶层功能</option>
                  {project?.features
                    .filter((f) => f.id !== featureForm.id)
                    .map((f) => (
                      <option key={f.id} value={f.id}>
                        {f.name}
                      </option>
                    ))}
                </select>
              </label>
              <div className="form-label">
                标识颜色
                <div className="color-picker">
                  {COLORS.map((color) => (
                    <button
                      key={color}
                      type="button"
                      aria-label={`选择颜色 ${color}`}
                      className={featureForm.color === color ? "selected" : ""}
                      style={{ background: color }}
                      onClick={() => setFeatureForm({ ...featureForm, color })}
                    >
                      {featureForm.color === color && <Check size={12} />}
                    </button>
                  ))}
                </div>
              </div>
            </div>
            <div className="modal-footer">
              <button
                type="button"
                className="button button-subtle"
                onClick={() => setFeatureForm(null)}
              >
                取消
              </button>
              <button
                type="submit"
                className="button button-primary"
                disabled={!!busy || !featureForm.name.trim()}
              >
                {busy === "保存功能" && <Loader2 size={14} className="spin" />}
                {featureForm.id ? "保存功能" : "创建功能"}
              </button>
            </div>
          </form>
        </Modal>
      )}

      {bindForm && (
        <Modal
          title={bindForm.anchorId ? "重新绑定代码" : "绑定代码片段"}
          subtitle="只保存阅读索引，不会修改代码文件。"
          onClose={() => setBindForm(null)}
        >
          <form onSubmit={submitBinding}>
            <div className="binding-preview">
              <FileCode2 size={19} />
              <div>
                <strong>
                  {bindForm.symbol?.name || filename(bindForm.range.file)}
                </strong>
                <span>{bindForm.range.file}</span>
                <small>
                  L{bindForm.range.startLine}–L{bindForm.range.endLine} ·{" "}
                  {bindForm.symbol ? "整个符号" : "选中片段"}
                </small>
              </div>
              <span className="binding-check">
                <Check size={14} />
              </span>
            </div>
            <label className="form-label">
              关联到功能
              <select
                value={bindForm.featureId}
                disabled={!!bindForm.anchorId}
                onChange={(e) =>
                  setBindForm({ ...bindForm, featureId: e.target.value })
                }
              >
                {project?.features.map((feature) => (
                  <option key={feature.id} value={feature.id}>
                    {feature.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="form-label">
              阅读备注（可选）
              <textarea
                rows={3}
                placeholder="这段代码在功能中负责什么？"
                value={bindForm.note}
                onChange={(e) =>
                  setBindForm({ ...bindForm, note: e.target.value })
                }
              />
            </label>
            <div className="modal-footer">
              <button
                type="button"
                className="button button-subtle"
                onClick={() => setBindForm(null)}
              >
                取消
              </button>
              <button
                type="submit"
                className="button button-primary"
                disabled={!!busy}
              >
                <Link2 size={14} />
                {bindForm.anchorId ? "确认重绑" : "确认绑定"}
              </button>
            </div>
          </form>
        </Modal>
      )}

      {deleteTarget && (
        <Modal
          title="删除这个功能索引？"
          subtitle="源码文件不会改变。此功能的绑定和阅读记录将被移除。"
          onClose={() => setDeleteTarget(null)}
        >
          <div className="delete-summary">
            <Layers size={20} />
            <div>
              <strong>{deleteTarget.name}</strong>
              <span>{deleteTarget.anchors.length} 个代码片段</span>
            </div>
          </div>
          <div className="modal-footer">
            <button
              className="button button-subtle"
              onClick={() => setDeleteTarget(null)}
            >
              保留功能
            </button>
            <button
              className="button button-danger"
              disabled={!!busy}
              onClick={() =>
                run("删除功能", async () => {
                  adopt(await api.deleteFeature(deleteTarget.id));
                  setDeleteTarget(null);
                  setNotice({ text: "功能索引已删除，源码未改变。" });
                })
              }
            >
              <Trash2 size={14} />
              删除索引
            </button>
          </div>
        </Modal>
      )}

      {report !== null && (
        <Modal
          title="阅读与审阅记录"
          subtitle="包含功能说明、代码锚点以及当前审阅状态。"
          onClose={() => setReport(null)}
          wide
        >
          <pre className="report-preview" tabIndex={0}>
            {report}
          </pre>
          <div className="modal-footer">
            <button
              className="button button-subtle"
              onClick={async () => {
                try {
                  if (!navigator.clipboard)
                    throw new Error("clipboard unavailable");
                  await navigator.clipboard.writeText(report);
                  setNotice({ text: "阅读记录已复制。" });
                } catch {
                  setNotice({
                    text: "当前环境无法访问剪贴板，可以在上方记录中选择并复制文本。",
                    error: true,
                  });
                }
              }}
            >
              <Copy size={14} />
              复制内容
            </button>
            {!api.isDesktop && (
              <button
                className="button button-primary"
                onClick={downloadReport}
              >
                <Download size={14} />
                下载 Markdown
              </button>
            )}
          </div>
        </Modal>
      )}

      {openHelp && (
        <Modal
          title="在桌面版打开本地仓库"
          subtitle="当前浏览器运行的是内置演示，无法直接访问你的磁盘或 Git。"
          onClose={() => setOpenHelp(false)}
        >
          <div className="desktop-help">
            <div className="desktop-help-icon">
              <FolderOpen size={29} />
            </div>
            <p>
              启动 <strong>CodeTrail 桌面版</strong>{" "}
              后，点击右上角“打开仓库”，选择项目文件夹，即可读取真实代码和 Git
              改动。
            </p>
            <div className="command-label">在工程目录中启动</div>
            <code>npm run tauri dev</code>
            <p className="help-footnote">
              需要已安装 Rust、Node.js 和系统构建依赖。完整步骤见源码包内
              README。
            </p>
          </div>
          <div className="modal-footer">
            <button
              className="button button-primary"
              onClick={() => setOpenHelp(false)}
            >
              继续体验演示
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

function SmallEmpty({
  icon,
  title,
  text,
  action,
}: {
  icon: React.ReactNode;
  title: string;
  text: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="small-empty">
      <span>{icon}</span>
      <strong>{title}</strong>
      <p>{text}</p>
      {action}
    </div>
  );
}
function EditorLoading() {
  return (
    <div className="editor-loading">
      <Loader2 size={20} className="spin" />
      <span>正在加载代码…</span>
    </div>
  );
}
function EditorError({ text, onRetry }: { text: string; onRetry: () => void }) {
  return (
    <div className="editor-error">
      <TriangleAlert size={28} />
      <h3>暂时无法显示这段代码</h3>
      <p>{text}</p>
      <button className="button button-subtle" onClick={onRetry}>
        <RefreshCw size={14} />
        重新读取
      </button>
    </div>
  );
}
function Modal({
  title,
  subtitle,
  children,
  onClose,
  wide = false,
}: {
  title: string;
  subtitle: string;
  children: React.ReactNode;
  onClose: () => void;
  wide?: boolean;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const root = dialogRef.current;
    const first =
      root?.querySelector<HTMLElement>("input, textarea, select") ||
      root?.querySelector<HTMLElement>("button");
    first?.focus();
    const trap = (event: KeyboardEvent) => {
      if (event.key !== "Tab" || !root) return;
      const elements = Array.from(
        root.querySelectorAll<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex="0"]',
        ),
      );
      const firstElement = elements[0];
      const lastElement = elements.at(-1);
      if (event.shiftKey && document.activeElement === firstElement) {
        event.preventDefault();
        lastElement?.focus();
      } else if (!event.shiftKey && document.activeElement === lastElement) {
        event.preventDefault();
        firstElement?.focus();
      }
    };
    root?.addEventListener("keydown", trap);
    return () => {
      root?.removeEventListener("keydown", trap);
      previous?.focus();
    };
  }, []);
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className={`modal ${wide ? "modal-wide" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        ref={dialogRef}
      >
        <div className="modal-heading">
          <div>
            <h2>{title}</h2>
            <p>{subtitle}</p>
          </div>
          <button
            className="icon-button"
            aria-label="关闭对话框"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}
