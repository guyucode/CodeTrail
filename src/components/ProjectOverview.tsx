import { ArrowUpRight, ChevronDown } from "lucide-react";
import type { AnchorBinding, Feature, ProjectSnapshot, ReadingStep } from "../types";

// Resolve a named evidence within its named feature; never silently choose another anchor.
export function overviewTarget(project: ProjectSnapshot, step: ReadingStep) {
  const feature = project.features.find(f => f.id === step.featureId);
  const anchor = feature?.anchors.find(a => a.id === step.evidenceId);
  if (!feature || !anchor || ["missing", "ambiguous"].includes(anchor.status)) return null;
  return { feature, anchor };
}

export default function ProjectOverview({ project, onRead }: {
  project: ProjectSnapshot;
  onRead: (anchor: AnchorBinding, feature: Feature) => void;
}) {
  const overview = project.overview;
  if (!overview) return null;
  return <section className="project-overview" aria-label="项目概览">
    <header><span className="eyebrow">先看全局，再读代码</span><h2>{project.name} 是怎样运行的</h2><p>{overview.summary}</p></header>
    <div className="overview-modules" aria-label="模块职责">
      {overview.modules.map(module => <div key={module.id}><h3>{module.name}</h3><p>{module.responsibility}</p></div>)}
    </div>
    <div className="overview-paths">
      {overview.paths.map((path, index) => <article className="overview-path" key={path.id}>
        <header><span className="path-number">0{index + 1}</span><h3>{path.name}</h3><p>{path.description}</p></header>
        <ol>{path.steps.map((step, stepIndex) => {
          const target = overviewTarget(project, step);
          const module = overview.modules.find(m => m.id === step.moduleId);
          return <li key={step.id}>
            <button disabled={!target} onClick={() => { if (target) onRead(target.anchor, target.feature); }}
              aria-label={`阅读步骤：${path.name} / ${step.title}`}>
              <span className="step-module">{stepIndex + 1} · {module?.name}</span>
              <strong>{step.title}<ArrowUpRight size={13} /></strong><span>{step.description}</span>
              {!target ? <small>关联需要更新</small> : target.feature.agentDelivery?.matchesSource === false || target.anchor.status === "changed" ? <small>证据已变化，阅读时请核对</small> : <small>进入代码</small>}
            </button>
            {stepIndex < path.steps.length - 1 && <ChevronDown className="path-arrow" size={14} />}
          </li>;
        })}</ol>
      </article>)}
    </div>
    <p className="overview-footnote">这是 Agent 维护的阅读路径，说明模块怎样配合；顺序不代表已验证的运行调用图。点击步骤查看对应功能和源码。</p>
  </section>;
}
