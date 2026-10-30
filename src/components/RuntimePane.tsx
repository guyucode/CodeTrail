import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
export interface RuntimeLocation {path:string;startLine:number;endLine:number;fileHash:string}
interface Value {name:string;value:string}
export interface Observation {id:string;title:string;status:string;result:string;reason:string;inputs:Value[];outputs:Value[];location:RuntimeLocation;elapsedMs:number}
interface Record {sequence:number;recordedAt:string;observation:Observation;sourceExcerpt:string}
interface Info {id:string;label:string;kind:'application'|'isolatedTest';producer:string;summary:string}
interface Summary {info:Info;status:string;startedAt:string;finishedAt:string|null;revision:number;stepCount:number}
export interface StepView {record:Record;sourceMatches:boolean;sourceProblem:string|null}
interface RunView {run:Omit<Summary,'stepCount'>&{steps:Record[]};steps:StepView[]}
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
    <header><span className="eyebrow">先看发生了什么，再看代码</span><h2>运行记录</h2><p>这里显示程序实际采集的中间结果；没有采集的步骤，不推测为已执行。</p></header>
    {error&&<p role="alert" className="runtime-warning">{error}</p>}
    {loading?<p>读取运行记录…</p>:!runs.length?<div className="runtime-empty"><h3>还没有运行记录</h3><p>需要让程序在关键位置写入记录。Agent 接入采集器后，每次运行都会有独立编号；阅读器不会自动执行项目。</p></div>:<>
      <label className="runtime-picker">选择一次运行<select aria-label="选择一次运行" value={selected} onChange={e=>{selectedRef.current=e.target.value;setSelected(e.target.value);setRecord(null);}}>{runs.map(r=><option key={r.info.id} value={r.info.id}>{time(r.startedAt)} · {r.info.label} · {status[r.status]||r.status}</option>)}</select></label>
      {record&&<>
        <div className="runtime-run-summary"><strong>{record.run.info.label}</strong><span>{record.run.info.kind==='isolatedTest'?'隔离测试执行':'应用执行'} · {status[record.run.status]||record.run.status}</span>
          <p>{record.run.info.summary}</p><small>采集来源：{record.run.info.producer} · 运行编号：{record.run.info.id}</small>
          {record.run.status==='running'&&<p>尚未收到结束记录；若进程已退出，已有步骤仍可查看，不据此断言仍在运行。</p>}
        </div>
        {!record.steps.length&&<p>这次运行还没有采集到步骤。</p>}
        <RuntimeSteps steps={record.steps} onCode={onCode}/>
      </>}
    </>}
  </section>;
}
