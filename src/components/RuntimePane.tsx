import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
export interface RuntimeLocation {path:string;startLine:number;endLine:number;fileHash:string}
interface Value {name:string;value:string}
export interface Observation {id:string;title:string;status:string;result:string;reason:string;inputs:Value[];outputs:Value[];location:RuntimeLocation;elapsedMs:number}
interface Record {sequence:number;recordedAt:string;observation:Observation;sourceExcerpt:string}
interface Info {id:string;label:string;kind:'application'|'isolatedTest';producer:string;summary:string}
interface Summary {info:Info;status:string;startedAt:string;finishedAt:string|null;revision:number;stepCount:number}
export interface StepView {record:Record;sourceMatches:boolean;sourceProblem:string|null}
export interface Verification {goal:string;inputs:Value[];expected:string;actual:string;keyValues:Value[];verdict:'matched'|'different'|'notCompared'}
export interface RunView {run:Omit<Summary,'stepCount'>&{steps:Record[];verification?:Verification|null};steps:StepView[]}
const status: {[key:string]:string}={running:'运行中',completed:'已结束',failed:'失败',cancelled:'已取消',ok:'已执行',error:'异常',skipped:'已跳过'};
const time=(value:string)=>new Date(Number(value)).toLocaleString();
const request=<T,>(root:string,action:string,payload:unknown={})=>invoke<T>('journal_action',{root,action,payload});
export function RuntimeSteps({steps,onCode}:{steps:StepView[];onCode:(location:RuntimeLocation)=>void}) {
  return <ol className="runtime-steps">{steps.map(({record,sourceMatches,sourceProblem})=>{
    const s=record.observation;
    return <li key={s.id} className={`runtime-step runtime-${s.status}`}>
      <div className="runtime-step-title"><span>{record.sequence}</span><h3>{s.title}</h3><small>{status[s.status]||s.status} · {s.elapsedMs} ms</small></div>
      <p className="runtime-result">{s.result}</p>
      <p className="runtime-reason">为什么：{s.reason}</p>
      <details><summary>展开关键变量与代码位置</summary>
        <div className="runtime-values">{[['输入',s.inputs],['输出',s.outputs]].map(([label,values])=><section key={String(label)}><h4>{String(label)}</h4>
          {(values as Value[]).length?<dl>{(values as Value[]).map(v=><div key={v.name}><dt>{v.name}</dt><dd>{v.value}</dd></div>)}</dl>:<p>此步未采集变量</p>}
        </section>)}</div>
        <p>{s.location.path} · L{s.location.startLine}–{s.location.endLine}</p>
        <small>记录接收时间：{time(record.recordedAt)} · SHA-256：{s.location.fileHash}</small>
        {!sourceMatches&&<p className="runtime-warning">当前源码与运行时版本不一致，保留当时记录。{sourceProblem}</p>}
        <button disabled={!sourceMatches} onClick={()=>onCode(s.location)}>查看对应代码</button>
        <details><summary>查看记录时的代码片段</summary><pre>{record.sourceExcerpt}</pre></details>
      </details>
    </li>;
  })}</ol>;
}
function Values({values}:{values:Value[]}) {
  return values.length ? <dl className="verification-values">{values.map(v=><div key={v.name}><dt>{v.name}</dt><dd>{v.value}</dd></div>)}</dl> : <span>未采集</span>;
}
export function VerificationOverview({record}:{record:RunView}) {
  const v=record.run.verification;
  const first=record.steps[0]?.record.observation;
  const last=record.steps.at(-1)?.record.observation;
  const verdict=record.run.status==='running'?'验证中':v ? {matched:'符合预期',different:'不符合预期',notCompared:'未比较'}[v.verdict] : `${status[record.run.status]||record.run.status} · 未记录预期`;
  const stale=record.steps.some(s=>!s.sourceMatches);
  return <article className="verification-overview" aria-label="验证摘要">
    <header><small>验证目标</small><h3>{v?.goal||record.run.info.label}</h3><span className={v?.verdict==='different'?'runtime-warning':''}>{record.run.status==='failed'&&v?'执行失败 · ':''}{verdict} · {record.run.info.kind==='isolatedTest'?'隔离验证':'应用执行'}</span></header>
    <dl className="verification-fields">
      <div><dt>输入</dt><dd><Values values={v?.inputs||first?.inputs.slice(0,5)||[]}/></dd></div>
      <div><dt>预期</dt><dd>{v?.expected||'未记录预期结果'}</dd></div>
      <div><dt>实际</dt><dd>{v?.actual||(record.run.status==='running'?'尚未收到最终验证摘要':last?.result||'未采集实际结果')}</dd></div>
      <div><dt>关键中间值</dt><dd><Values values={v?.keyValues||last?.outputs.slice(0,5)||[]}/></dd></div>
    </dl>
    {!v&&<p className="runtime-legacy">旧记录保留原运行名称、首步输入和末步结果；缺少预期时不推断验证通过。其余数据在下方过程里。</p>}
    {stale&&<p className="runtime-warning">对应源码已有变化，以上保留当时的结果；查看过程可核对历史片段。</p>}
  </article>;
}
export function RuntimeReading({record,onCode}:{record:RunView;onCode:(location:RuntimeLocation)=>void}) {
  return <>
    <VerificationOverview record={record}/>
    <details className="runtime-details"><summary>查看过程与代码</summary>
      <p>{record.run.info.summary}</p><small>来源：{record.run.info.producer} · 编号：{record.run.info.id}</small>
      {record.run.status==='running'&&<p>尚未收到结束记录；不据此断言进程仍在运行。</p>}
      {!record.steps.length&&<p>没有已采集的步骤。</p>}
      <RuntimeSteps steps={record.steps} onCode={onCode}/>
    </details>
  </>;
}
export default function RuntimePane({root,demo,onCode,onContext}:{root:string;demo:boolean;onCode:(location:RuntimeLocation)=>void;onContext:(value:string)=>void}) {
  const [runs,setRuns]=useState<Summary[]>([]);const [selected,setSelected]=useState('');
  const [record,setRecord]=useState<RunView|null>(null);const [error,setError]=useState('');const [loading,setLoading]=useState(true);
  const selectedRef=useRef(selected);selectedRef.current=selected;
  const contextRef=useRef(onContext);contextRef.current=onContext;
  useEffect(()=>{
    if(demo){setLoading(false);return;}
    let alive=true,busy=false;
    async function refresh(){if(busy)return;busy=true;
      try{
        const next=await request<Summary[]>(root,'traceList');if(!alive)return;setRuns(next);
        const id=selectedRef.current||next[0]?.info.id||'';
        if(id){if(!selectedRef.current){selectedRef.current=id;setSelected(id);}
          const view=await request<RunView>(root,'traceRead',{runId:id});if(!alive||selectedRef.current!==id)return;
          setRecord(view);contextRef.current(`运行记录：${view.run.info.label} / ${id}`);
        } else {setRecord(null);contextRef.current('运行记录');}
        setError('');
      }catch(e){if(alive)setError(String(e));}finally{busy=false;if(alive)setLoading(false);}
    }
    void refresh();const timer=setInterval(()=>void refresh(),2000);return()=>{alive=false;clearInterval(timer);};
  },[root,demo,selected]);
  return <section className="runtime-pane" aria-label="本次运行">
    <header><h2>运行记录</h2><p>看这次验证的输入、预期和实际结果。</p></header>
    {error&&<p role="alert" className="runtime-warning">{error}</p>}
    {loading?<p>读取运行记录…</p>:!runs.length?<div className="runtime-empty"><h3>还没有运行记录</h3><p>请让 Agent 完成一次真实验证并保存记录，之后就能在这里回看结果。</p></div>:<>
      <label className="runtime-picker">选择一次运行<select aria-label="选择一次运行" value={selected} onChange={e=>{selectedRef.current=e.target.value;setSelected(e.target.value);setRecord(null);}}>{runs.map(r=><option key={r.info.id} value={r.info.id}>{time(r.startedAt)} · {r.info.label} · {status[r.status]||r.status}</option>)}</select></label>
      {record&&<RuntimeReading record={record} onCode={onCode}/>}

    </>}
  </section>;
}
