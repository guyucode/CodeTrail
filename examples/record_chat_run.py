"""Execute the existing chat backend against a disposable database and DEMO model.

No service listener, no model downloads, no external inference. Original project
sources are not edited: per-instance wrappers observe real function returns.
"""
from __future__ import annotations

import argparse
import asyncio
import functools
import json
from pathlib import Path
import sys
import tempfile
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "sdk" / "python"))
from codetrail_trace import Recorder, location


def observe_async(owner, name, recorder, root, title, describe):
    original = getattr(owner, name)
    @functools.wraps(original)
    async def observed(*args, **kwargs):
        source = location(root, original)
        begin = time.perf_counter()
        try:
            result = await original(*args, **kwargs)
        except Exception as exc:
            recorder.step(title, "此函数抛出 " + type(exc).__name__, "实际执行进入异常路径；原异常继续向上传递。",
                          source=source, status="error", outputs={"exception_type": type(exc).__name__},
                          elapsed_ms=(time.perf_counter()-begin)*1000)
            raise
        outcome, reason, inputs, outputs = describe(args, kwargs, result)
        recorder.step(title, outcome, reason, source=source, inputs=inputs, outputs=outputs,
                      elapsed_ms=(time.perf_counter()-begin)*1000)
        return result
    setattr(owner, name, observed)


def observe_sync(owner, name, recorder, root, title, describe):
    original = getattr(owner, name)
    @functools.wraps(original)
    def observed(*args, **kwargs):
        source = location(root, original)
        begin = time.perf_counter()
        result = original(*args, **kwargs)
        outcome, reason, inputs, outputs = describe(args, kwargs, result)
        recorder.step(title, outcome, reason, source=source, inputs=inputs, outputs=outputs,
                      elapsed_ms=(time.perf_counter()-begin)*1000)
        return result
    setattr(owner, name, observed)


async def execute(args):
    root = Path(args.project).resolve()
    sys.path.insert(0, str(root))
    from workbench.config import Settings, Deployment, Options
    from workbench.service import Service
    recorder = Recorder(root, args.cli, "发送“你好”的后台实际运行" if not args.invalid else "实际异常：不支持的任务类型",
                        kind="isolatedTest", producer="record_chat_run.py / 原项目 Service + SQLite + DEMO",
                        summary="真实执行聊天后台；空测试数据库、DEMO 脚本模型。未执行浏览器代码、未调用真实模型。", strict=True)
    with tempfile.TemporaryDirectory(prefix="codetrail-chat-") as directory:
        home = Path(directory)
        model = Deployment(id="demo", label="DEMO", backend="demo", model="scripted-demo",
                           context_window=8192, max_output_tokens=1024, resource_group="demo", remote=False)
        service = Service(Settings(home, db_path=home/"test.db", deployments=[model]))
        started = False
        try:
            await service.start()
            started = True
            session = await service.db("new_session", dict(title="运行记录隔离测试", model_id="demo", runtime="direct", business="chat",
                agent_id=None, system_prompt="Be accurate.", pinned_json=[], participant_models={}, learner_id=None,
                allow_remote=False, options_json=Options(max_output_tokens=1024).model_dump()))
            original_submit = service.submit
            request_id = uuid.uuid4().hex
            recorder.step("准备调用真实后台", "测试会话已建立，即将提交“你好”。", "本次选择 direct、普通聊天、DEMO 模型；不经过网页输入处理。",
                          source=location(root, original_submit), inputs={"sid":session["id"],"text":"你好","request_id":request_id,"kind":"invalid" if args.invalid else "chat"},
                          outputs={"runtime":session["runtime"],"backend":model.backend})
            observe_sync(service.store, "request_run", recorder, root, "识别是否为重复请求", lambda a,k,r:(
                "找到了已有任务。" if r else "没有旧任务，进入新任务准入。", "查询结果决定复用旧任务，还是继续校验并创建新任务。",
                {"request_id":a[1],"text":a[2]}, {"existing_run_id":r["id"] if r else None}))
            observe_async(service, "_admit_validated", recorder, root, "准入校验与任务创建返回", lambda a,k,r:(
                "准入函数返回了任务记录。", "实际校验会话、模型和运行方式后，返回持久化任务；后台可能已并发执行后续步骤。",
                {"sid":a[0],"kind":a[3]}, {"run_id":r["id"],"observed_status":r["status"]}))
            observe_async(service, "direct_context", recorder, root, "整理模型实际输入", lambda a,k,r:(
                f"选出了 {len(r)} 条模型输入消息。", "运行方式是 direct；函数按预算选取完整历史，再加入约束和本次问题。",
                {"text":a[3],"runtime":a[0]["runtime"]}, {"message_count":len(r),"roles":[m["role"] for m in r],"last_content":r[-1]["content"]}))
            observe_async(service.gateway, "generate", recorder, root, "DEMO 调用实际返回", lambda a,k,r:(
                "收到了脚本模型返回的文本。", "实际走 backend=demo 分支；这不是 AI 推理结果，也没有远程调用。",
                {"model":a[0].model,"backend":a[0].backend,"input_message_count":len(a[1])},
                {"text":r.text,"finish_reason":r.finish_reason,"input_tokens":r.input_tokens,"output_tokens":r.output_tokens}))
            observe_sync(service.store, "finish_run", recorder, root, "把任务终态写入数据库", lambda a,k,r:(
                "终态事务已返回："+str(r), "以数据库实际返回状态为准；同一事务处理清理和终态事件。",
                {"run_id":a[0],"requested_status":a[1]}, {"stored_status":r}))
            observe_async(service, "submit", recorder, root, "提交函数返回 / 异常", lambda a,k,r:(
                "已拿到后台任务号。", "返回任务号表示准入完成，不表示模型已经生成完毕。",
                {"text":a[1],"request_id":a[2]}, {"run_id":r["id"],"observed_status":r["status"]}))
            try:
                run = await service.submit(session["id"], "你好", request_id, "invalid" if args.invalid else "chat")
            except ValueError:
                if not args.invalid:
                    raise
                recorder.finish("failed", "实际提交 invalid 类型，Service.submit 抛出 ValueError；没有创建任务或调用模型。")
                return recorder.id
            task = service.tasks.get(run["id"])
            if task:
                await task
            final = await service.db("run", run["id"])
            page = await service.db("event_page", run["id"], 0)
            event_page = service.store.event_page
            recorder.step("读取持久事件与最后结果", f"数据库状态为 {final['status']}，读取到 {len(page['events'])} 条事件。",
                          "执行结束后实际读取 SQLite；这是事件读取验证，没有启动浏览器或 SSE 网络连接。",
                          source=location(root,event_page), inputs={"after":0,"run_id":run["id"]},
                          outputs={"status":final["status"],"event_count":len(page["events"]),"event_types":[e["type"] for e in page["events"]]})
            recorder.finish("completed" if final["status"]=="completed" else "failed",
                            f"真实后台执行结束：{final['status']}。采集消息准入、上下文、DEMO 调用和终态；未执行浏览器、未连接真实模型。")
            return recorder.id
        except BaseException:
            if not recorder.closed:
                recorder.finish("failed", "采集验证意外中断，已采集步骤保留；详细异常只在本机终端检查。")
            raise
        finally:
            if started:
                await service.close()


if __name__ == "__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--cli", required=True)
    parser.add_argument("--invalid", action="store_true")
    print(json.dumps({"runId":asyncio.run(execute(parser.parse_args()))},ensure_ascii=False))
