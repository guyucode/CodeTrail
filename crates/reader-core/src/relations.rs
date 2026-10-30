//! Evidence-backed, bounded Python source bindings. These are syntactic relationships,
//! not a claim that a call executes at runtime or that Python's dynamic dispatch is known.
use crate::types::*;
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use tree_sitter::{Node, Parser};

// Bound a single AST batch and retained query results, not the sum of unrelated files.
const MAX_RELATIONS: usize = 50_000;
const MAX_DEPTH: usize = 256;

/// Capture current source and return a local, one-hop relationship view.
/// Historical diffs never borrow this worktree graph to infer historical callers.
pub fn analyze(
    root: &Path,
    symbol_id: &str,
    base: &str,
    head: &str,
    semantic: bool,
) -> Result<SymbolRelations> {
    let root = crate::canonical_root(root)?;
    let mut scan = crate::scan_repository(&root)?;
    let symbol = scan
        .symbols
        .iter()
        .find(|s| s.id == symbol_id)
        .cloned()
        .context(
            "该符号在当前 WORKTREE 中不存在；删除或重命名后的历史调用者未知，请刷新并选择当前符号",
        )?;
    let contents: BTreeMap<_, _> = scan
        .contents
        .iter()
        .map(|(p, c)| (p.clone(), c.clone()))
        .collect();
    let file_hashes = fingerprints(&contents);
    let source_digest = crate::digest(&serde_json::to_vec(&file_hashes)?);
    let mut features = crate::load_features(&root, &mut scan.warnings)?;
    for feature in &mut features {
        for anchor in &mut feature.anchors {
            crate::resolve_anchor(anchor, &scan.symbols, &scan.contents);
        }
    }
    let snapshot = ProjectSnapshot {
        overview: None,
        root: root.to_string_lossy().into_owned(),
        name: root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        git: crate::git::info(&root),
        files: scan.files,
        symbols: scan.symbols,
        features,
        warnings: scan.warnings,
    };
    let mut warnings = snapshot.warnings.clone();
    let selected_python = crate::parse::language_for(&symbol.range.file) == "python";
    let change_summary = if head == "WORKTREE" {
        Some(crate::git::changes(&root, base, head, &snapshot)?)
    } else {
        None
    };
    let impact_targets: BTreeSet<String> = change_summary
        .iter()
        .flat_map(|summary| &summary.files)
        .flat_map(|file| file.affected_symbols.iter().cloned())
        .collect();
    let mut graph = PythonGraph::capture(&contents, &snapshot.symbols, &mut warnings)?;
    let (mut relations, blocked_relation_ids) = graph.resolve(
        &contents,
        &snapshot.symbols,
        &symbol,
        &impact_targets,
        semantic,
        &mut warnings,
    )?;
    let unsupported: BTreeSet<_> = snapshot
        .symbols
        .iter()
        .map(|s| crate::parse::language_for(&s.range.file))
        .filter(|language| *language != "python")
        .collect();
    if !unsupported.is_empty() {
        warnings.push(format!(
            "关系分析目前仅覆盖 Python；{} 的符号/差异可阅读，但不推断调用关系。",
            unsupported.into_iter().collect::<Vec<_>>().join("、")
        ));
    }
    warnings.push("语法关系表示源码名称绑定，不证明运行时执行；动态派发、反射、跨语言调用和自定义 sys.path 不作猜测。".into());
    let mut engine = RelationEngine {
        name: "Python 静态绑定".into(),
        status: if relations
            .iter()
            .any(|r| r.kind == RelationKind::Call && r.resolution != RelationResolution::Resolved)
            || graph.partial
        {
            RelationEngineStatus::Partial
        } else {
            RelationEngineStatus::Ready
        },
        detail:
            "全项目声明绑定 + 逐文件调用/读取位置 + 仓库根目录及 src 导入路径；独立保留当前符号关系，未开启语言服务。"
                .into(),
    };
    if selected_python && semantic {
        let report = crate::semantic::enrich_with_policy(
            &root,
            &contents,
            &snapshot.symbols,
            &mut relations,
            &symbol,
            &blocked_relation_ids,
        );
        engine = report.engine;
        warnings.extend(report.warnings);
        if graph.partial && engine.status == RelationEngineStatus::Ready {
            engine.status = RelationEngineStatus::Partial;
            engine
                .detail
                .push_str(" 静态证据仍有截断或不完整范围，语言服务结果不消除此限制。");
        }
    }
    graph.explain_decorated_targets(&mut relations, &symbol, &mut warnings);
    if !selected_python {
        engine = RelationEngine {
            name: "Python 关系引擎".into(),
            status: RelationEngineStatus::Unavailable,
            detail: "选中符号不是 Python；当前版本不提供其他语言的关系推断。".into(),
        };
    }

    let (changed_symbol_ids, impacts) = if head == "WORKTREE" {
        let summary = change_summary
            .as_ref()
            .expect("WORKTREE comparison captured");
        warnings.extend(summary.warnings.iter().cloned());
        let changed: BTreeSet<String> = summary
            .files
            .iter()
            .flat_map(|f| f.affected_symbols.iter().cloned())
            .collect();
        let current: BTreeSet<_> = snapshot.symbols.iter().map(|s| &s.id).collect();
        let historical_count = changed.iter().filter(|id| !current.contains(id)).count();
        if historical_count > 0 {
            warnings.push(format!("{historical_count} 个旧符号已删除或重命名；其历史调用者未知，当前 WORKTREE 关系不能代替旧版本关系。"));
        }
        let impacts = feature_impacts(&snapshot.features, summary, &changed, &relations);
        (changed.into_iter().collect(), impacts)
    } else {
        warnings.push("当前关系图仅基于 WORKTREE；所选比较包含历史版本，不使用当前调用图推断历史影响，已关闭变更节点标记与 Feature 影响推断。".into());
        (Vec::new(), Vec::new())
    };
    // All displayed symbols, AST sites, LSP facts and Git associations must refer
    // to the captured content. Include additions/removals in this check, not only
    // the selected file: a new module may change import ambiguity.
    validate_capture(&root, &file_hashes)?;
    let incoming = relations
        .iter()
        .filter(|r| {
            r.kind == RelationKind::Call && r.target_symbol_id.as_deref() == Some(symbol_id)
        })
        .cloned()
        .collect();
    let outgoing = relations
        .iter()
        .filter(|r| {
            r.kind == RelationKind::Call && r.source_symbol_id.as_deref() == Some(symbol_id)
        })
        .cloned()
        .collect();
    let references = relations
        .iter()
        .filter(|r| {
            r.kind == RelationKind::Reference && r.target_symbol_id.as_deref() == Some(symbol_id)
        })
        .cloned()
        .collect();
    warnings.sort();
    warnings.dedup();
    Ok(SymbolRelations {
        symbol,
        source_digest,
        file_hashes,
        incoming,
        outgoing,
        references,
        changed_symbol_ids,
        impacts,
        engine,
        base: base.into(),
        head: head.into(),
        warnings,
    })
}

fn fingerprints(contents: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    contents
        .iter()
        .map(|(p, c)| (p.clone(), crate::digest(c.as_bytes())))
        .collect()
}

fn validate_capture(root: &Path, captured: &BTreeMap<String, String>) -> Result<()> {
    let current = crate::scan_repository(root)?;
    let current: BTreeMap<_, _> = current
        .contents
        .iter()
        .map(|(p, c)| (p.clone(), crate::digest(c.as_bytes())))
        .collect();
    ensure!(
        current == *captured,
        "关系分析期间源码已变化；已丢弃过期结果，请刷新后重新分析"
    );
    Ok(())
}

fn feature_impacts(
    features: &[Feature],
    changes: &ChangeSummary,
    changed: &BTreeSet<String>,
    relations: &[CodeRelation],
) -> Vec<FeatureImpact> {
    let direct: BTreeSet<_> = changes
        .files
        .iter()
        .flat_map(|f| f.feature_ids.iter())
        .collect();
    let mut impacts = Vec::new();
    for feature in features {
        if direct.contains(&feature.id) {
            impacts.push(FeatureImpact {
                feature_id: feature.id.clone(),
                kind: FeatureImpactKind::Direct,
                reason: "现有 Git 行级差异与该功能的符号/范围锚点直接关联；请查看前后版本。".into(),
                evidence: Vec::new(),
            });
            continue;
        }
        let evidence: Vec<_> = relations
            .iter()
            .filter(|relation| {
                relation.kind == RelationKind::Call
                    && relation.resolution == RelationResolution::Resolved
                    && relation
                        .target_symbol_id
                        .as_ref()
                        .is_some_and(|id| changed.contains(id))
                    && relation.source_symbol_id.is_some()
                    && feature.anchors.iter().any(|anchor| {
                        !matches!(
                            anchor.status,
                            AnchorStatus::Missing | AnchorStatus::Ambiguous
                        ) && (crate::contains_range(&anchor.range, &relation.site)
                            || (anchor.kind == AnchorKind::Symbol
                                && anchor.symbol_id == relation.source_symbol_id))
                    })
            })
            .cloned()
            .collect();
        if !evidence.is_empty() {
            impacts.push(FeatureImpact {
                feature_id: feature.id.clone(), kind: FeatureImpactKind::Possible,
                reason: "该功能的代码直接调用了本次变更符号，可能受影响；仅扩展一跳，不代表行为错误，也不自动改写审阅状态。".into(),
                evidence,
            });
        }
    }
    impacts.sort_by(|a, b| a.feature_id.cmp(&b.feature_id));
    impacts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Module,
    Class,
    Function,
    Lambda,
    Comprehension,
}

#[derive(Debug, Clone)]
struct Scope {
    parent: Option<usize>,
    kind: ScopeKind,
    source_id: Option<String>,
    source_name: String,
    owners: Vec<String>,
    bindings: BTreeMap<String, Vec<Binding>>,
    wildcard: bool,
    unreliable: bool,
    mutations: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct Binding {
    kind: BindingKind,
    end_byte: usize,
    conditional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BindingKind {
    Symbol(String),
    Module { name: String, loaded: String },
    Member { module: String, name: String },
    Shadow(String),
}

#[derive(Debug, Clone)]
enum Expr {
    Name(String),
    Attribute(Box<Expr>, String),
    Dynamic(String),
}

#[derive(Debug, Clone)]
struct Usage {
    kind: RelationKind,
    scope: usize,
    expression: Expr,
    target_name: String,
    site: CodeRange,
    column: u32,
    start_byte: usize,
    end_byte: usize,
}

struct Module {
    path: String,
    package: String,
    scopes: Vec<Scope>,
    usages: Vec<Usage>,
    attribute_writes: Vec<AttributeWrite>,
    decorated_symbols: BTreeSet<String>,
    partial: bool,
    usage_limit_reached: bool,
}

struct AttributeWrite {
    scope: usize,
    name: String,
    site: CodeRange,
}

struct PythonGraph<'a> {
    modules: Vec<Module>,
    module_names: BTreeMap<String, Vec<usize>>,
    namespace_names: BTreeSet<String>,
    symbols: HashMap<&'a str, &'a Symbol>,
    mutated_modules: BTreeSet<String>,
    mutated_symbols: BTreeSet<String>,
    decorated_symbols: BTreeSet<String>,
    partial: bool,
}

#[derive(Debug, Clone)]
enum Value {
    Symbol(String),
    Module {
        name: String,
        loaded: BTreeSet<String>,
    },
    Unknown(RelationResolution, String),
    /// A known source write invalidates the original object binding. Language
    /// service definition/reference locations must not revive that binding.
    Invalidated(String),
}

impl<'a> PythonGraph<'a> {
    fn capture(
        contents: &BTreeMap<String, String>,
        symbols: &'a [Symbol],
        warnings: &mut Vec<String>,
    ) -> Result<Self> {
        let mut graph = Self {
            modules: Vec::new(),
            module_names: BTreeMap::new(),
            namespace_names: BTreeSet::new(),
            symbols: symbols.iter().map(|s| (s.id.as_str(), s)).collect(),
            mutated_modules: BTreeSet::new(),
            mutated_symbols: BTreeSet::new(),
            decorated_symbols: BTreeSet::new(),
            partial: false,
        };
        let mut symbols_by_file = grouped_symbols(symbols.iter());
        for (path, content) in contents
            .iter()
            .filter(|(p, _)| crate::parse::language_for(p) == "python")
        {
            // Collect declarations and writes for every module before resolving imports.
            // No usage budget is consumed by this pass.
            let module = capture_module(
                path,
                content,
                symbols_by_file.remove(path.as_str()).unwrap_or_default(),
                false,
            )?;
            if module.partial {
                graph.partial = true;
                warnings.push(format!(
                    "{path} 的关系声明分析不完整（语法错误或深度限制）；不据此断言不存在调用者。"
                ));
            }
            let names = module_names(path);
            let index = graph.modules.len();
            graph
                .decorated_symbols
                .extend(module.decorated_symbols.iter().cloned());
            for name in names {
                let mut prefix = name.as_str();
                while let Some((parent, _)) = prefix.rsplit_once('.') {
                    graph.namespace_names.insert(parent.into());
                    prefix = parent;
                }
                graph.module_names.entry(name).or_default().push(index);
            }
            graph.modules.push(module);
        }
        graph.invalidate_attribute_writes(warnings);
        Ok(graph)
    }

    /// Attribute assignment does not introduce a new lexical binding. Resolve
    /// the receiver's owner only after all scopes have been collected, then
    /// invalidate that owner and equivalent imported module aliases. A parameter
    /// with the same name is a distinct owner and must not poison a global import.
    fn invalidate_attribute_writes(&mut self, warnings: &mut Vec<String>) {
        let mut owners = BTreeSet::new();
        let mut modules = BTreeSet::new();
        let mut symbols = BTreeSet::new();
        let mut evidence = Vec::new();
        for (module_index, module) in self.modules.iter().enumerate() {
            for write in &module.attribute_writes {
                let Some(owner) = self.binding_owner(module_index, write.scope, &write.name) else {
                    continue;
                };
                owners.insert((module_index, owner, write.name.clone()));
                let bindings = &module.scopes[owner].bindings[&write.name];
                // Do not evaluate expressions or follow arbitrary assignments.
                // Only already-established lexical/import bindings are followed.
                match self.binding_value(
                    module_index,
                    owner,
                    bindings,
                    None,
                    &mut BTreeSet::new(),
                    0,
                ) {
                    Value::Module { name, .. } => {
                        modules.insert(name.clone());
                        evidence.push(format!(
                            "{} L{}：{} → 模块 {name}",
                            write.site.file, write.site.start_line, write.name
                        ));
                    }
                    Value::Symbol(id) => {
                        symbols.insert(id);
                    }
                    _ => {}
                }
            }
        }
        for (module, scope, name) in owners {
            self.modules[module].scopes[scope].mutations.insert(name);
        }
        self.mutated_modules = modules;
        self.mutated_symbols = symbols;
        if !evidence.is_empty() {
            warnings.push(format!("已观察到导入模块属性写入（{}）。按模块整体保守失效跨函数和导入别名关系，保留未知并禁止语言服务恢复原目标；不推断这些写入是否实际执行。", evidence.into_iter().take(12).collect::<Vec<_>>().join("；")));
        }
        if self
            .modules
            .iter()
            .any(|module| !module.attribute_writes.is_empty())
        {
            warnings.push("属性写入追踪限于可证明的词法绑定和显式导入；参数、容器、赋值别名及反射产生的间接对象关联未建立，不能据此断言没有其他动态替换。".into());
        }
    }

    fn binding_owner(&self, module: usize, mut scope: usize, name: &str) -> Option<usize> {
        loop {
            let current = &self.modules[module].scopes[scope];
            if current.bindings.contains_key(name) {
                return Some(scope);
            }
            let mut parent = current.parent?;
            while self.modules[module].scopes[parent].kind == ScopeKind::Class {
                parent = self.modules[module].scopes[parent].parent?;
            }
            scope = parent;
        }
    }

    fn module_is_mutated(&self, module: &str) -> bool {
        self.mutated_modules
            .iter()
            .any(|changed| module == changed || module.starts_with(&format!("{changed}.")))
    }

    fn explain_decorated_targets(
        &self,
        relations: &mut [CodeRelation],
        focus: &Symbol,
        warnings: &mut Vec<String>,
    ) {
        if self.decorated_symbols.is_empty() {
            return;
        }
        warnings.push(format!("本次索引有 {} 个带装饰器的定义；@decorator 可以替换函数/类的可调用对象。已解析边只绑定源码声明，不声称装饰后的运行时目标唯一，也不补造隐式装饰器调用。", self.decorated_symbols.len()));
        if self.decorated_symbols.contains(&focus.id) {
            warnings.push(format!("选中声明 {}（{} L{}）带装饰器；请连同声明上方的 @decorator 阅读，实际执行对象可能已被替换。", focus.qualified_name, focus.range.file, focus.range.start_line));
        }
        for relation in relations {
            if let Some(symbol) = relation
                .target_symbol_id
                .as_deref()
                .filter(|id| self.decorated_symbols.contains(*id))
                .and_then(|id| self.symbols.get(id))
            {
                relation.detail.push_str(&format!(" 目标声明 {} L{} 带 @decorator，装饰器可能替换可调用对象；此边绑定声明，不保证实际运行时目标。", symbol.range.file, symbol.range.start_line));
            }
        }
    }

    fn resolve(
        &mut self,
        contents: &BTreeMap<String, String>,
        symbols: &[Symbol],
        focus: &Symbol,
        impact_targets: &BTreeSet<String>,
        semantic: bool,
        warnings: &mut Vec<String>,
    ) -> Result<(Vec<CodeRelation>, BTreeSet<String>)> {
        let mut relations = Vec::new();
        let mut auxiliary = Vec::new();
        let mut blocked = BTreeSet::new();
        let mut symbols_by_file = grouped_symbols(symbols.iter());
        let mut query_truncated = false;
        let mut auxiliary_truncated = false;
        for (index, module) in self.modules.iter().enumerate() {
            // Hold at most one file's AST sites. Bindings remain complete across files.
            let batch = capture_module(
                &module.path,
                &contents[&module.path],
                symbols_by_file
                    .remove(module.path.as_str())
                    .unwrap_or_default(),
                true,
            )?;
            if batch.usage_limit_reached {
                self.partial = true;
                warnings.push(format!("{} 的关系位置超过单文件 {MAX_RELATIONS} 上限；仅该文件的调用/引用位置不完整，其他文件继续独立分析。", module.path));
            }
            for usage in &batch.usages {
                let value = self.expression(
                    index,
                    usage.scope,
                    &usage.expression,
                    usage,
                    &mut BTreeSet::new(),
                    0,
                );
                let invalidated = matches!(&value, Value::Invalidated(_));
                let (resolution, target_id, target, mut detail) = match value {
                    Value::Symbol(id) => match self.symbols.get(id.as_str()) {
                        Some(symbol) => (
                            RelationResolution::Resolved,
                            Some(id.clone()),
                            Some(symbol.range.clone()),
                            if symbol.kind == "class" {
                                "词法/导入绑定到当前源码类定义；不推断 __init__、__new__ 或运行时结果。".into()
                            } else {
                                "依据词法作用域与精确导入路径绑定到当前源码定义；不是运行时执行证明。".into()
                            },
                        ),
                        None => (
                            RelationResolution::Unresolved,
                            None,
                            None,
                            "绑定目标未在当前符号索引中，需刷新。".into(),
                        ),
                    },
                    Value::Module { name, .. } => (
                        RelationResolution::Unresolved,
                        None,
                        None,
                        format!("{name} 绑定到模块；不将模块属性或可调用性猜测为函数。"),
                    ),
                    Value::Unknown(status, detail) => (status, None, None, detail),
                    Value::Invalidated(detail) => {
                        (RelationResolution::Unresolved, None, None, detail)
                    }
                };
                let scope = &module.scopes[usage.scope];
                if scope.source_id.is_none() {
                    if matches!(scope.kind, ScopeKind::Lambda | ScopeKind::Comprehension) {
                        detail
                            .push_str(" 位置属于匿名/延迟执行域，不把它归为外层函数体的直接调用。");
                    } else {
                        detail.push_str(
                            " 位置属于模块顶层或声明求值位置，不归属所声明函数的函数体。",
                        );
                    }
                }
                let identity = format!(
                    "{}\0{}\0{}\0{}\0{}\0{}",
                    module.path,
                    usage.start_byte,
                    usage.end_byte,
                    if usage.kind == RelationKind::Call {
                        "call"
                    } else {
                        "reference"
                    },
                    scope.source_id.as_deref().unwrap_or(""),
                    usage.target_name
                );
                let id = format!("rel:{}", crate::digest(identity.as_bytes()));
                let relevant = scope.source_id.as_deref() == Some(focus.id.as_str())
                    || target_id.as_deref() == Some(focus.id.as_str())
                    || (usage.kind == RelationKind::Call
                        && target_id
                            .as_ref()
                            .is_some_and(|id| impact_targets.contains(id)));
                let destination = if relevant {
                    if relations.len() >= MAX_RELATIONS {
                        query_truncated = true;
                        continue;
                    }
                    &mut relations
                } else {
                    if !semantic {
                        continue;
                    }
                    if auxiliary.len() >= MAX_RELATIONS {
                        auxiliary_truncated = true;
                        continue;
                    }
                    &mut auxiliary
                };
                if invalidated {
                    blocked.insert(id.clone());
                }
                destination.push(CodeRelation {
                    id,
                    kind: usage.kind.clone(),
                    source_symbol_id: scope.source_id.clone(),
                    source_name: scope.source_name.clone(),
                    target_symbol_id: target_id,
                    target_name: usage.target_name.clone(),
                    site: usage.site.clone(),
                    site_column: usage.column,
                    target,
                    resolution,
                    origin: RelationOrigin::Syntax,
                    detail,
                });
            }
        }
        if query_truncated {
            self.partial = true;
            warnings.push(format!("当前符号或变更影响的关系结果超过 {MAX_RELATIONS} 上限；已截断结果，不能据此断言其他关系不存在。"));
        }
        if auxiliary_truncated {
            self.partial = true;
            warnings.push(format!("Pyright 辅助候选超过 {MAX_RELATIONS} 个位置；当前符号的静态关系独立保留，语言服务仅增强已收录候选。"));
        }
        relations.extend(auxiliary);
        relations.sort_by(|a, b| {
            (&a.site.file, a.site.start_line, a.site_column, &a.id).cmp(&(
                &b.site.file,
                b.site.start_line,
                b.site_column,
                &b.id,
            ))
        });
        Ok((relations, blocked))
    }

    fn expression(
        &self,
        module: usize,
        scope: usize,
        expr: &Expr,
        usage: &Usage,
        seen: &mut BTreeSet<(usize, usize, String)>,
        depth: usize,
    ) -> Value {
        if depth > 64 {
            return unknown("导入/作用域追踪超过限制，未猜测目标");
        }
        match expr {
            Expr::Name(name) => self.lookup(module, scope, name, Some(usage), seen, depth + 1),
            Expr::Attribute(object, name) => {
                match self.expression(module, scope, object, usage, seen, depth + 1) {
                    Value::Module {
                        name: module_name,
                        loaded,
                    } => self.module_member(&module_name, name, &loaded, false, seen, depth + 1),
                    Value::Unknown(status, detail) => Value::Unknown(status, detail),
                    Value::Invalidated(detail) => Value::Invalidated(detail),
                    Value::Symbol(_) => {
                        unknown("类/实例/函数属性的接收者与派发需要语义证据；静态层不按方法名连线")
                    }
                }
            }
            Expr::Dynamic(text) => unknown(&format!(
                "动态调用表达式 {text}；下标、返回值、lambda 或其他运行时目标未解析"
            )),
        }
    }

    fn lookup(
        &self,
        module: usize,
        mut scope: usize,
        name: &str,
        usage: Option<&Usage>,
        seen: &mut BTreeSet<(usize, usize, String)>,
        depth: usize,
    ) -> Value {
        if depth > 64 {
            return unknown("导入/闭包追踪超过限制，未猜测目标");
        }
        loop {
            let current = &self.modules[module].scopes[scope];
            if current.unreliable || self.modules[module].scopes[0].unreliable {
                return unknown("当前作用域含语法错误或动态执行，名称绑定不可靠");
            }
            if current.wildcard {
                return unknown("作用域含通配符导入，不能可靠确定名称绑定");
            }
            if current.mutations.contains(name) {
                return Value::Invalidated("已观察到该名称所属对象的属性写入，原绑定可能被动态替换；跨函数/别名保持未知，语言服务不升级为原目标".into());
            }
            if let Some(bindings) = current.bindings.get(name) {
                let key = (module, scope, name.to_owned());
                if !seen.insert(key.clone()) {
                    return unknown("循环导入/重导出，未选择未经证明的目标");
                }
                let result = self.binding_value(module, scope, bindings, usage, seen, depth + 1);
                seen.remove(&key);
                return result;
            }
            let Some(mut parent) = current.parent else {
                break;
            };
            // Class namespaces are not lexical closures for methods or nested classes.
            while self.modules[module].scopes[parent].kind == ScopeKind::Class {
                let Some(next) = self.modules[module].scopes[parent].parent else {
                    break;
                };
                parent = next;
            }
            scope = parent;
        }
        if is_builtin(name) {
            Value::Unknown(
                RelationResolution::External,
                format!("{name} 是 Python 内置名称，当前项目内没有目标定义"),
            )
        } else {
            unknown(&format!(
                "未在词法作用域或显式导入中找到 {name}；不按全局同名符号猜测"
            ))
        }
    }

    fn binding_value(
        &self,
        module: usize,
        scope: usize,
        bindings: &[Binding],
        usage: Option<&Usage>,
        seen: &mut BTreeSet<(usize, usize, String)>,
        depth: usize,
    ) -> Value {
        if let Some(reason) = bindings.iter().find_map(|b| {
            if let BindingKind::Shadow(reason) = &b.kind {
                Some(reason)
            } else {
                None
            }
        }) {
            return unknown(reason);
        }
        if bindings.iter().any(|b| b.conditional) {
            return Value::Unknown(
                if bindings.len() > 1 {
                    RelationResolution::Ambiguous
                } else {
                    RelationResolution::Unresolved
                },
                "名称在条件分支/异常处理内绑定，运行时可用目标不确定".into(),
            );
        }
        if let Some(usage) = usage {
            // Same-scope evaluation before a def/import cannot use that new binding.
            // A function's global/closure lookup is deferred, so do not compare its
            // body offset with a declaration in an enclosing scope.
            let immediate = usage.scope == scope || !self.has_deferred_scope(module, usage.scope);
            if immediate && bindings.iter().all(|b| b.end_byte > usage.start_byte) {
                return unknown("调用/读取发生在该作用域的定义或导入绑定之前，不能确定可用目标");
            }
        }
        if let BindingKind::Module { name, .. } = &bindings[0].kind {
            if bindings.iter().all(
                |b| matches!(&b.kind, BindingKind::Module { name: other, .. } if other == name),
            ) {
                let loaded = bindings
                    .iter()
                    .filter_map(|b| {
                        if let BindingKind::Module { loaded, .. } = &b.kind {
                            Some(loaded.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                return self.module_value(name, loaded);
            }
        }
        if bindings.iter().any(|b| b.kind != bindings[0].kind) {
            return Value::Unknown(
                RelationResolution::Ambiguous,
                "同一词法名称存在多个定义或导入绑定；未按出现顺序猜测运行时目标".into(),
            );
        }
        match &bindings[0].kind {
            BindingKind::Symbol(id) => {
                let module_written = module_names(&self.modules[module].path)
                    .iter()
                    .any(|name| self.module_is_mutated(name));
                if self.mutated_symbols.contains(id) || module_written {
                    Value::Invalidated(
                        "定义所属对象/模块存在已观察到的属性写入；原声明不能作为当前运行时唯一目标"
                            .into(),
                    )
                } else {
                    Value::Symbol(id.clone())
                }
            }
            BindingKind::Module { name, loaded } => {
                self.module_value(name, BTreeSet::from([loaded.clone()]))
            }
            BindingKind::Member { module, name } => {
                self.module_member(module, name, &BTreeSet::new(), true, seen, depth + 1)
            }
            BindingKind::Shadow(reason) => unknown(reason),
        }
    }

    fn has_deferred_scope(&self, module: usize, mut scope: usize) -> bool {
        loop {
            let current = &self.modules[module].scopes[scope];
            if matches!(
                current.kind,
                ScopeKind::Function | ScopeKind::Lambda | ScopeKind::Comprehension
            ) {
                return true;
            }
            let Some(parent) = current.parent else {
                return false;
            };
            scope = parent;
        }
    }

    fn module_value(&self, name: &str, loaded: BTreeSet<String>) -> Value {
        if self
            .module_names
            .get(name)
            .is_some_and(|indices| indices.len() > 1)
        {
            return Value::Unknown(
                RelationResolution::Ambiguous,
                format!("模块 {name} 对应多个源码文件/源码根；没有选择第一个同名结果"),
            );
        }
        if self.module_exists(name) {
            Value::Module {
                name: name.into(),
                loaded,
            }
        } else {
            Value::Unknown(
                RelationResolution::External,
                format!(
                    "模块 {name} 不在本次索引内（外部依赖或未配置的源码根）；没有项目内定义证据"
                ),
            )
        }
    }

    fn module_exists(&self, name: &str) -> bool {
        self.module_names.contains_key(name) || self.namespace_names.contains(name)
    }

    fn module_member(
        &self,
        module: &str,
        name: &str,
        loaded: &BTreeSet<String>,
        import_member: bool,
        seen: &mut BTreeSet<(usize, usize, String)>,
        depth: usize,
    ) -> Value {
        if depth > 64 {
            return unknown("循环/过深的导入路径，未猜测目标");
        }
        if self.module_is_mutated(module) {
            return Value::Invalidated(format!("已观察到导入模块 {module} 的属性写入；按模块整体保守失效跨函数/别名关系，语言服务不恢复原声明绑定"));
        }
        if let Value::Unknown(status, detail) = self.module_value(module, loaded.clone()) {
            return Value::Unknown(status, detail);
        }
        if let Some(indices) = self.module_names.get(module) {
            let index = indices[0];
            let scope = &self.modules[index].scopes[0];
            if scope.bindings.contains_key(name) || scope.wildcard || scope.unreliable {
                return self.lookup(index, 0, name, None, seen, depth + 1);
            }
        }
        let child = format!("{module}.{name}");
        if (import_member
            || loaded
                .iter()
                .any(|path| path == &child || path.starts_with(&format!("{child}."))))
            && self.module_exists(&child)
        {
            let mut loaded = loaded.clone();
            loaded.insert(child.clone());
            return self.module_value(&child, loaded);
        }
        unknown(&format!(
            "模块 {module} 未提供可证明的 {name} 绑定；未导入的子模块/动态属性不自动连线"
        ))
    }
}

fn unknown(detail: &str) -> Value {
    Value::Unknown(RelationResolution::Unresolved, detail.into())
}

fn module_names(path: &str) -> Vec<String> {
    let without_extension = path.rsplit_once('.').map(|(p, _)| p).unwrap_or(path);
    let module = without_extension
        .strip_suffix("/__init__")
        .unwrap_or(without_extension)
        .replace('/', ".");
    let mut names = vec![module.clone()];
    if let Some(name) = module.strip_prefix("src.") {
        names.push(name.to_owned());
    }
    names
}

type FileSymbols<'a> = BTreeMap<(String, u32), &'a Symbol>;

fn grouped_symbols<'a>(
    symbols: impl IntoIterator<Item = &'a Symbol>,
) -> BTreeMap<&'a str, FileSymbols<'a>> {
    let mut grouped: BTreeMap<&str, FileSymbols<'a>> = BTreeMap::new();
    for symbol in symbols {
        grouped.entry(&symbol.range.file).or_default().insert(
            (symbol.qualified_name.clone(), symbol.range.end_line),
            symbol,
        );
    }
    grouped
}

fn capture_module<'a>(
    path: &str,
    content: &'a str,
    symbols: FileSymbols<'a>,
    record_usages: bool,
) -> Result<Module> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
    let tree = parser
        .parse(content, None)
        .context("Python 关系解析未完成")?;
    let names = module_names(path);
    let package = if path.ends_with("/__init__.py") || path.ends_with("/__init__.pyi") {
        names[0].clone()
    } else {
        names[0]
            .rsplit_once('.')
            .map(|(p, _)| p)
            .unwrap_or("")
            .to_owned()
    };
    let mut builder = Builder {
        module: Module {
            path: path.to_owned(),
            package,
            scopes: vec![Scope {
                parent: None,
                kind: ScopeKind::Module,
                source_id: None,
                source_name: format!("{path} · 模块顶层"),
                owners: Vec::new(),
                bindings: BTreeMap::new(),
                wildcard: false,
                unreliable: tree.root_node().has_error(),
                mutations: BTreeSet::new(),
            }],
            usages: Vec::new(),
            attribute_writes: Vec::new(),
            decorated_symbols: BTreeSet::new(),
            partial: tree.root_node().has_error(),
            usage_limit_reached: false,
        },
        content,
        symbols,
        remaining: MAX_RELATIONS,
        record_usages,
    };
    builder.visit(tree.root_node(), 0, false, 0);
    Ok(builder.module)
}

struct Builder<'a> {
    module: Module,
    content: &'a str,
    symbols: BTreeMap<(String, u32), &'a Symbol>,
    remaining: usize,
    record_usages: bool,
}

impl Builder<'_> {
    fn text(&self, node: Node<'_>) -> &str {
        self.content.get(node.byte_range()).unwrap_or("")
    }

    fn bind(
        &mut self,
        scope: usize,
        name: &str,
        kind: BindingKind,
        node: Node<'_>,
        conditional: bool,
    ) {
        if !name.is_empty() {
            self.module.scopes[scope]
                .bindings
                .entry(name.to_owned())
                .or_default()
                .push(Binding {
                    kind,
                    end_byte: node.end_byte(),
                    conditional,
                });
        }
    }

    fn shadow(&mut self, scope: usize, node: Node<'_>, reason: &str) {
        let mut pending = vec![(node, 0)];
        while let Some((node, depth)) = pending.pop() {
            if depth > MAX_DEPTH {
                self.module.partial = true;
                self.module.scopes[scope].unreliable = true;
                continue;
            }
            match node.kind() {
                "identifier" => {
                    let name = self.text(node).to_owned();
                    self.bind(
                        scope,
                        &name,
                        BindingKind::Shadow(reason.into()),
                        node,
                        false,
                    );
                }
                "attribute" | "subscript" => {
                    if let Some(root) = root_identifier(node) {
                        let name = self.text(root).to_owned();
                        self.module.scopes[scope].mutations.insert(name.clone());
                        self.module.attribute_writes.push(AttributeWrite {
                            scope,
                            name,
                            site: CodeRange {
                                file: self.module.path.clone(),
                                start_line: node.start_position().row as u32 + 1,
                                end_line: end_line(node),
                            },
                        });
                    }
                }
                _ => {
                    let mut cursor = node.walk();
                    pending.extend(
                        node.named_children(&mut cursor)
                            .map(|child| (child, depth + 1)),
                    );
                }
            }
        }
    }

    fn child_scope(
        &mut self,
        parent: usize,
        kind: ScopeKind,
        symbol: Option<&Symbol>,
        name: String,
    ) -> usize {
        let mut owners = self.module.scopes[parent].owners.clone();
        if let Some(symbol) = symbol {
            owners.push(symbol.name.clone());
        }
        let index = self.module.scopes.len();
        self.module.scopes.push(Scope {
            parent: Some(parent),
            kind,
            source_id: symbol.map(|s| s.id.clone()),
            source_name: name,
            owners,
            bindings: BTreeMap::new(),
            wildcard: false,
            unreliable: false,
            mutations: BTreeSet::new(),
        });
        index
    }

    fn visit(&mut self, node: Node<'_>, scope: usize, conditional: bool, depth: usize) {
        if depth > MAX_DEPTH {
            self.module.partial = true;
            self.module.scopes[scope].unreliable = true;
            return;
        }
        match node.kind() {
            "function_definition" | "class_definition" => {
                let Some(name_node) = node.child_by_field_name("name") else {
                    return;
                };
                let name = self.text(name_node).to_owned();
                let mut qualified = self.module.scopes[scope].owners.clone();
                qualified.push(name.clone());
                let qualified = qualified.join("::");
                let symbol = self
                    .symbols
                    .get(&(qualified.clone(), end_line(node)))
                    .filter(|symbol| {
                        symbol.range.start_line <= node.start_position().row as u32 + 1
                    })
                    .copied()
                    .cloned();
                let kind = symbol
                    .as_ref()
                    .map(|s| BindingKind::Symbol(s.id.clone()))
                    .unwrap_or_else(|| BindingKind::Shadow("定义未与当前符号索引唯一对应".into()));
                if node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "decorated_definition")
                {
                    if let Some(symbol) = &symbol {
                        self.module.decorated_symbols.insert(symbol.id.clone());
                    }
                }
                self.bind(scope, &name, kind, node, conditional);
                let scope_kind = if node.kind() == "function_definition" {
                    ScopeKind::Function
                } else {
                    ScopeKind::Class
                };
                let child = self.child_scope(scope, scope_kind, symbol.as_ref(), qualified);
                if let Some(parameters) = node.child_by_field_name("parameters") {
                    self.parameters(parameters, scope, child, depth + 1);
                }
                for field in ["superclasses", "return_type", "type_parameters"] {
                    if let Some(value) = node.child_by_field_name(field) {
                        self.visit(value, scope, conditional, depth + 1);
                    }
                }
                if let Some(body) = node.child_by_field_name("body") {
                    self.visit(body, child, false, depth + 1);
                }
            }
            "lambda" => {
                let child = self.child_scope(
                    scope,
                    ScopeKind::Lambda,
                    None,
                    format!(
                        "{} · lambda L{}",
                        self.module.path,
                        node.start_position().row + 1
                    ),
                );
                if let Some(parameters) = node.child_by_field_name("parameters") {
                    self.parameters(parameters, scope, child, depth + 1);
                }
                if let Some(body) = node.child_by_field_name("body") {
                    self.visit(body, child, false, depth + 1);
                }
            }
            "list_comprehension"
            | "set_comprehension"
            | "dictionary_comprehension"
            | "generator_expression" => {
                let child = self.child_scope(
                    scope,
                    ScopeKind::Comprehension,
                    None,
                    format!(
                        "{} · comprehension L{}",
                        self.module.path,
                        node.start_position().row + 1
                    ),
                );
                if node.kind() != "generator_expression" {
                    self.module.scopes[child].source_id =
                        self.module.scopes[scope].source_id.clone();
                    self.module.scopes[child].source_name =
                        self.module.scopes[scope].source_name.clone();
                }
                let mut cursor = node.walk();
                let children: Vec<_> = node.named_children(&mut cursor).collect();
                let mut first = true;
                for part in &children {
                    if part.kind() == "for_in_clause" {
                        if let Some(left) = part.child_by_field_name("left") {
                            self.shadow(
                                child,
                                left,
                                "推导式局部变量遮蔽外层同名定义，运行时目标未知",
                            );
                        }
                        if let Some(right) = part.child_by_field_name("right") {
                            self.visit(
                                right,
                                if first { scope } else { child },
                                conditional,
                                depth + 1,
                            );
                        }
                        first = false;
                    }
                }
                for part in children {
                    if part.kind() != "for_in_clause" {
                        self.visit(part, child, conditional, depth + 1);
                    }
                }
            }
            "import_statement" => self.import(node, scope, conditional, false),
            "import_from_statement" => self.import(node, scope, conditional, true),
            "assignment" | "augmented_assignment" => {
                if let Some(left) = node.child_by_field_name("left") {
                    self.shadow(scope, left, "该名称是局部赋值/重新绑定，值的调用目标未知");
                    self.target_reads(left, scope, conditional, depth + 1);
                }
                for field in ["right", "type"] {
                    if let Some(value) = node.child_by_field_name(field) {
                        self.visit(value, scope, conditional, depth + 1);
                    }
                }
            }
            "named_expression" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.shadow(scope, name, "赋值表达式重新绑定了该名称，调用目标未知");
                    // A walrus inside a comprehension can bind its containing scope.
                    if self.module.scopes[scope].kind == ScopeKind::Comprehension {
                        if let Some(parent) = self.module.scopes[scope].parent {
                            self.shadow(parent, name, "推导式赋值表达式可能重新绑定该名称");
                        }
                    }
                }
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(value, scope, conditional, depth + 1);
                }
            }
            "for_statement" | "for_in_clause" => {
                if let Some(left) = node.child_by_field_name("left") {
                    self.shadow(scope, left, "循环变量遮蔽了同名定义，调用目标未知");
                }
                for field in ["right", "body", "alternative"] {
                    if let Some(value) = node.child_by_field_name(field) {
                        self.visit(value, scope, true, depth + 1);
                    }
                }
            }
            "as_pattern" => {
                let alias = node.child_by_field_name("alias");
                if let Some(alias) = alias {
                    self.shadow(scope, alias, "with/except/as 绑定的运行时目标未知");
                }
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if Some(child) != alias {
                        self.visit(child, scope, conditional, depth + 1);
                    }
                }
            }
            "except_clause" => {
                if let Some(alias) = node.child_by_field_name("alias") {
                    self.shadow(scope, alias, "异常处理变量的调用目标未知");
                }
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if Some(child) != node.child_by_field_name("alias") {
                        self.visit(child, scope, true, depth + 1);
                    }
                }
            }
            "case_clause" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "case_pattern" {
                        self.shadow(scope, child, "模式匹配可能绑定同名局部变量；未猜测动态目标");
                    } else {
                        self.visit(child, scope, true, depth + 1);
                    }
                }
            }
            "global_statement" | "nonlocal_statement" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.shadow(
                        scope,
                        child,
                        "global/nonlocal 名称可能被跨作用域写入，静态层保持未知",
                    );
                    if node.kind() == "global_statement" {
                        self.shadow(0, child, "名称受到 global 声明/写入影响，需语义证据核对");
                    } else {
                        let mut parent = self.module.scopes[scope].parent;
                        while let Some(index) = parent {
                            if self.module.scopes[index].kind == ScopeKind::Function {
                                self.shadow(
                                    index,
                                    child,
                                    "名称受到 nonlocal 声明/写入影响，需语义证据核对",
                                );
                                break;
                            }
                            parent = self.module.scopes[index].parent;
                        }
                    }
                }
            }
            "delete_statement" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.shadow(scope, child, "名称可能被删除，不能确定当前绑定");
                    self.target_reads(child, scope, conditional, depth + 1);
                }
            }
            "call" => {
                if let Some(function) = node.child_by_field_name("function") {
                    self.usage(function, scope, RelationKind::Call);
                    if function.kind() == "identifier" && self.text(function) == "exec" {
                        self.module.scopes[scope].unreliable = true;
                    }
                    self.callee_reads(function, scope, conditional, depth + 1);
                }
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    self.visit(arguments, scope, conditional, depth + 1);
                }
            }
            "attribute" => {
                self.usage(node, scope, RelationKind::Reference);
                if let Some(object) = node.child_by_field_name("object") {
                    self.visit(object, scope, conditional, depth + 1);
                }
            }
            "identifier" => self.usage(node, scope, RelationKind::Reference),
            "keyword_argument" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(value, scope, conditional, depth + 1);
                }
            }
            "type_alias_statement" => {
                if let Some(left) = node.child_by_field_name("left") {
                    self.shadow(scope, left, "类型别名不是已证明的可调用目标");
                }
                if let Some(right) = node.child_by_field_name("right") {
                    self.visit(right, scope, conditional, depth + 1);
                }
            }
            _ => {
                let conditional = conditional
                    || matches!(
                        node.kind(),
                        "if_statement"
                            | "elif_clause"
                            | "else_clause"
                            | "try_statement"
                            | "while_statement"
                            | "with_statement"
                    );
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.visit(child, scope, conditional, depth + 1);
                }
            }
        }
    }

    fn parameters(&mut self, node: Node<'_>, outer: usize, inner: usize, depth: usize) {
        let mut cursor = node.walk();
        for parameter in node.named_children(&mut cursor) {
            match parameter.kind() {
                "default_parameter" | "typed_default_parameter" => {
                    if let Some(name) = parameter.child_by_field_name("name") {
                        self.shadow(
                            inner,
                            name,
                            "函数参数遮蔽同名定义；调用目标由运行时入参决定",
                        );
                    }
                    for field in ["value", "type"] {
                        if let Some(value) = parameter.child_by_field_name(field) {
                            self.visit(value, outer, false, depth + 1);
                        }
                    }
                }
                "typed_parameter" => {
                    let ty = parameter.child_by_field_name("type");
                    let mut subcursor = parameter.walk();
                    for child in parameter.named_children(&mut subcursor) {
                        if Some(child) == ty {
                            self.visit(child, outer, false, depth + 1);
                        } else {
                            self.shadow(
                                inner,
                                child,
                                "带类型标注的参数仍是运行时入参；静态层不猜测调用目标",
                            );
                        }
                    }
                }
                _ => self.shadow(
                    inner,
                    parameter,
                    "函数/lambda 参数遮蔽同名定义，调用目标未知",
                ),
            }
        }
    }

    fn import(&mut self, node: Node<'_>, scope: usize, conditional: bool, from: bool) {
        let module = if from {
            let raw = node
                .child_by_field_name("module_name")
                .map(|n| self.text(n))
                .unwrap_or("");
            self.absolute_import(raw)
        } else {
            Some(String::new())
        };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "wildcard_import" {
                self.module.scopes[scope].wildcard = true;
                continue;
            }
            if Some(child) == node.child_by_field_name("module_name") {
                continue;
            }
            let (imported, alias) = if child.kind() == "aliased_import" {
                (
                    child
                        .child_by_field_name("name")
                        .map(|n| self.text(n))
                        .unwrap_or("")
                        .to_owned(),
                    child
                        .child_by_field_name("alias")
                        .map(|n| self.text(n).to_owned()),
                )
            } else if child.kind() == "dotted_name" {
                (self.text(child).to_owned(), None)
            } else {
                continue;
            };
            let local = alias
                .clone()
                .unwrap_or_else(|| imported.split('.').next().unwrap_or("").to_owned());
            let kind = if from {
                match &module {
                    Some(module) => BindingKind::Member {
                        module: module.clone(),
                        name: imported,
                    },
                    None => BindingKind::Shadow("相对导入超出已知包根，无法定位目标".into()),
                }
            } else {
                BindingKind::Module {
                    name: if alias.is_some() {
                        imported.clone()
                    } else {
                        local.clone()
                    },
                    loaded: imported,
                }
            };
            self.bind(scope, &local, kind, node, conditional);
        }
    }

    fn absolute_import(&self, raw: &str) -> Option<String> {
        let dots = raw.chars().take_while(|c| *c == '.').count();
        if dots == 0 {
            return Some(raw.into());
        }
        let mut package: Vec<_> = self
            .module
            .package
            .split('.')
            .filter(|s| !s.is_empty())
            .collect();
        if dots > package.len() {
            return None;
        }
        for _ in 1..dots {
            package.pop();
        }
        if !raw[dots..].is_empty() {
            package.extend(raw[dots..].split('.'));
        }
        Some(package.join("."))
    }

    fn target_reads(&mut self, node: Node<'_>, scope: usize, conditional: bool, depth: usize) {
        if depth > MAX_DEPTH {
            self.module.partial = true;
            self.module.scopes[scope].unreliable = true;
            return;
        }
        match node.kind() {
            "attribute" => {
                if let Some(object) = node.child_by_field_name("object") {
                    self.visit(object, scope, conditional, depth + 1);
                }
            }
            "subscript" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.visit(child, scope, conditional, depth + 1);
                }
            }
            "identifier" => {}
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.target_reads(child, scope, conditional, depth + 1);
                }
            }
        }
    }

    fn callee_reads(&mut self, node: Node<'_>, scope: usize, conditional: bool, depth: usize) {
        if depth > MAX_DEPTH {
            self.module.partial = true;
            self.module.scopes[scope].unreliable = true;
            return;
        }
        match node.kind() {
            "identifier" => {}
            "attribute" => {
                if let Some(object) = node.child_by_field_name("object") {
                    self.visit(object, scope, conditional, depth + 1);
                }
            }
            "parenthesized_expression" => {
                if let Some(child) = node.named_child(0) {
                    self.callee_reads(child, scope, conditional, depth + 1);
                }
            }
            _ => self.visit(node, scope, conditional, depth + 1),
        }
    }

    fn usage(&mut self, node: Node<'_>, scope: usize, kind: RelationKind) {
        if !self.record_usages {
            return;
        }
        if self.remaining == 0 {
            self.module.usage_limit_reached = true;
            return;
        }
        self.remaining -= 1;
        let token = evidence_token(node);
        let start = token.start_position();
        let line_start = token.start_byte().saturating_sub(start.column);
        let column = self
            .content
            .get(line_start..token.start_byte())
            .unwrap_or("")
            .encode_utf16()
            .count() as u32
            + 1;
        self.module.usages.push(Usage {
            kind,
            scope,
            expression: expression(node, self.content),
            target_name: self.text(node).chars().take(180).collect(),
            site: CodeRange {
                file: self.module.path.clone(),
                start_line: start.row as u32 + 1,
                end_line: end_line(token),
            },
            column,
            start_byte: token.start_byte(),
            end_byte: node.end_byte(),
        });
    }
}

fn expression(node: Node<'_>, content: &str) -> Expr {
    expression_at_depth(node, content, 0)
}

fn expression_at_depth(node: Node<'_>, content: &str, depth: usize) -> Expr {
    if depth > MAX_DEPTH {
        return Expr::Dynamic("表达式嵌套超过分析上限".into());
    }
    match node.kind() {
        "identifier" => Expr::Name(content.get(node.byte_range()).unwrap_or("").into()),
        "attribute" => match (
            node.child_by_field_name("object"),
            node.child_by_field_name("attribute"),
        ) {
            (Some(object), Some(attribute)) => Expr::Attribute(
                Box::new(expression_at_depth(object, content, depth + 1)),
                content.get(attribute.byte_range()).unwrap_or("").into(),
            ),
            _ => Expr::Dynamic("不完整的属性表达式".into()),
        },
        "parenthesized_expression" => node
            .named_child(0)
            .map(|child| expression_at_depth(child, content, depth + 1))
            .unwrap_or_else(|| Expr::Dynamic("空括号".into())),
        _ => Expr::Dynamic(
            content
                .get(node.byte_range())
                .unwrap_or("")
                .chars()
                .take(100)
                .collect(),
        ),
    }
}

fn evidence_token(mut node: Node<'_>) -> Node<'_> {
    for _ in 0..MAX_DEPTH {
        match node.kind() {
            "attribute" => return node.child_by_field_name("attribute").unwrap_or(node),
            "parenthesized_expression" => {
                if let Some(child) = node.named_child(0) {
                    node = child;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    node
}

fn root_identifier(mut node: Node<'_>) -> Option<Node<'_>> {
    for _ in 0..MAX_DEPTH {
        node = match node.kind() {
            "identifier" => return Some(node),
            "attribute" => node.child_by_field_name("object")?,
            "subscript" => node.child_by_field_name("value")?,
            _ => return None,
        };
    }
    None
}

fn end_line(node: Node<'_>) -> u32 {
    let end = node.end_position();
    if end.column == 0 {
        end.row.max(node.start_position().row + 1) as u32
    } else {
        end.row as u32 + 1
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "abs"
            | "all"
            | "any"
            | "ascii"
            | "bin"
            | "bool"
            | "breakpoint"
            | "bytearray"
            | "bytes"
            | "callable"
            | "chr"
            | "classmethod"
            | "compile"
            | "complex"
            | "delattr"
            | "dict"
            | "dir"
            | "divmod"
            | "enumerate"
            | "eval"
            | "exec"
            | "filter"
            | "float"
            | "format"
            | "frozenset"
            | "getattr"
            | "globals"
            | "hasattr"
            | "hash"
            | "help"
            | "hex"
            | "id"
            | "input"
            | "int"
            | "isinstance"
            | "issubclass"
            | "iter"
            | "len"
            | "list"
            | "locals"
            | "map"
            | "max"
            | "memoryview"
            | "min"
            | "next"
            | "object"
            | "oct"
            | "open"
            | "ord"
            | "pow"
            | "print"
            | "property"
            | "range"
            | "repr"
            | "reversed"
            | "round"
            | "set"
            | "setattr"
            | "slice"
            | "sorted"
            | "staticmethod"
            | "str"
            | "sum"
            | "super"
            | "tuple"
            | "type"
            | "vars"
            | "zip"
            | "__import__"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn snapshot_validation_rejects_changed_added_and_removed_source() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.py"), "def a():\n    return 1\n").unwrap();
        let contents = BTreeMap::from([("a.py".into(), "def a():\n    return 1\n".into())]);
        let captured = fingerprints(&contents);
        validate_capture(dir.path(), &captured).unwrap();
        fs::write(dir.path().join("a.py"), "def a():\n    return 2\n").unwrap();
        assert!(validate_capture(dir.path(), &captured).is_err());
        fs::write(dir.path().join("a.py"), &contents["a.py"]).unwrap();
        fs::write(dir.path().join("b.py"), "def a():\n    return 1\n").unwrap();
        assert!(validate_capture(dir.path(), &captured).is_err());
        fs::remove_file(dir.path().join("b.py")).unwrap();
        fs::remove_file(dir.path().join("a.py")).unwrap();
        assert!(validate_capture(dir.path(), &captured).is_err());
    }
}
