# 运行记录 v1：由程序采集，再回到代码

运行记录回答“这一回实际输入什么、走了哪一步、得到什么”。它不同于 Agent 工作动态，也不同于静态功能说明。CodeTrail 不执行项目、不补猜未采集变量、不把记录当成远程模型已验证。

## 阅读方式

打开项目后，左侧点“运行记录”，选择一次运行。默认看验证目标、输入、预期、实际和几个关键中间值；完整步骤、来源和代码位置放在“查看过程与代码”中。旧记录缺少预期时明确显示未记录。源码 SHA-256 相同时可跳到当前代码，再点“返回本次运行”。源码不同或文件丢失时禁用跳转，保留历史变量与当时的代码片段，避免错配旧行号。

右侧继续显示 Agent 动态与批注。运行页面的批注上下文包含运行名称和编号，仍由人手动导出与发送。新增运行不会自动抢占已选中的旧运行；当前选择每 2 秒更新。没有结束记录只说明未收到结束，不保证进程仍存活。

## 三个采集接口

CLI 与桌面共用核心存储，所有命令输出 `{ "ok": true, "value": ... }`；错误退出 1，输入最多 2 MiB。桌面只开放读取，不启动程序。

```text
reader-cli trace start ROOT  < start.json
reader-cli trace append ROOT < step.json
reader-cli trace finish ROOT < finish.json
reader-cli trace list ROOT
reader-cli trace read ROOT   < {"runId":"..."}
```

start JSON：`{id,label,kind,producer,summary}`。kind 必须为 `application`（应用执行）或 `isolatedTest`（隔离测试）；来源与替身服务范围须在 summary 明说。id 为 1–128 字节字母、数字、连字符、下划线，其他类型拒绝未知字段。label/producer 最多 200 字符，summary 最多 2000 字符。

append JSON：

```json
{
  "runId":"run-001",
  "step":{
    "id":"step-001",
    "title":"选择历史消息",
    "status":"ok",
    "result":"本次选入 2 条消息。",
    "reason":"当前是 direct 运行方式，按预算组合消息。",
    "inputs":[{"name":"runtime","value":"direct"}],
    "outputs":[{"name":"message_count","value":"2"}],
    "elapsedMs":3,
    "location":{"path":"service.py","startLine":1,"endLine":10,"fileHash":"采集调用前取得的真实 SHA-256"}
  }
}
```

step.status 为 ok/error/skipped；title 最多 200 字符，result/reason 最多 2000 字符，输入和输出各最多 30 个变量，name 最多 100 字符且组内唯一，value 为字符串、最多 2000 字符。不自动采集全部变量。location 必须在项目内并指向当前 SHA-256，1–200 行、片段最多 16 KiB；拒绝 .reader、.git 内部文件和符号链接。只验证来源文件版本与位置，不证明生产者确实执行了这段代码。

finish JSON：`{runId,status,summary}`，status 为 completed/failed/cancelled。completed 表示生产者报告此次执行结束，不等于业务被人工接受或测试充分；界面显示“已结束”。空步骤不能标记 completed。步骤序号与接收时间由核心生成，按采集到达顺序记录；并发步骤可能交错，这不是完整调用图。耗时包含采集影响，不当作无侵入性能基准。

## Python 接入

标准库采集器在 `sdk/python/codetrail_trace.py`。显式启用并传入可信本机 CLI 路径，不修改 Python 全局环境。默认采集失败只给出可见警告，业务继续、记录可能不完整；strict=True 仅适合隔离验证。以下保留旧接口示例；新的交付还应使用下方 verification 摘要填写预期和实际比较。

```python
from codetrail_trace import Recorder, location
recorder = Recorder(project_root, cli_path, "处理一条消息",
    summary="实际应用执行；这里只采集允许记录的字段。")
source = location(project_root, process_message)  # 调用前取得位置和哈希
result = process_message(text)
recorder.step("处理消息", "函数已返回。", "根据本次真实返回值记录。",
    source=source, inputs={"message_length":len(text)}, outputs={"result_length":len(result)})
recorder.finish("completed", "本次处理结束。")
```

应在异常路径记录明确失败，在关闭时完成记录；进程突然终止时保留 running 和已有步骤。不自动清理、不编造完成。异步应用按需要用 asyncio.to_thread 将同步 CLI 调用移出主事件循环。采集器会隐藏若干已知敏感变量名，并对超长值明确标注截断；这不是通用脱敏保证。必须使用字段允许清单，默认不记录密钥、环境变量、HTTP 头、完整用户历史或不必要的正文。

## 本次真实执行样例

`examples/record_chat_run.py` 在临时目录创建空 SQLite 数据库，只针对本次 Service 实例包装真实方法，调用原项目的 Service.submit、任务准入、direct_context、Gateway.generate、Store.finish_run 和 event_page。未修改原项目业务源码。每次调用创建独立运行编号。

```text
具备原项目核心依赖的 Python examples/record_chat_run.py --project /path/to/local_model_chat --cli /path/to/reader-cli
# 实际触发 INVALID_RUN_KIND，记录异常分支：
同一 Python examples/record_chat_run.py --project /path/to/local_model_chat --cli /path/to/reader-cli --invalid
```

真实执行的是后台 Python 与 SQLite。模型强制为原项目自带 DEMO 脚本实现，不连接真实推理或付费 API；没有启动网页、HTTP 监听器或测试 SSE 网络。采集的模型返回文本就是实际脚本输出，界面明确标注“隔离测试执行”。这些限制不能被“已结束”标签掩盖。启动器的 strict 模式开销会影响执行耗时，不能拿来做性能比较。

## 保存与边界

单文件原子保存 `.reader/runs.json`（schemaVersion:1，runs 数组），与 agent.json、activity.json、annotations.json、人工阅读确认完全隔离。缺少文件视为空，未知版本或损坏文件显式报错且不覆盖。初期开发的无 schemaVersion 记录按 v1 兼容读取。

单个存储最多 2 MiB、50 次运行、每次 200 步；超限拒绝新写入并提示备份归档，不自动删除历史。重复运行/步骤 ID、已结束运行的追加或二次结束均拒绝；trace.lock 串行跨进程写入，异常残留需确认无进程后人工清理。运行记录默认被项目 .gitignore 排除，不跟随源码提交。

此协议是可信本机采集器之间的约定，不是防伪签名：Agent/其他有文件权限的程序也能构造记录。必须注明生产者、测试替身和执行范围；不能用手填 JSON 冒充本次真实执行。

## 简约验证摘要（可选扩展，旧记录保留）

运行页面默认只看五项：验证目标、输入、预期、实际、关键中间值。完整步骤、来源、代码位置与旧源码片段折叠到“查看过程与代码”。缺少摘要的旧记录不迁移、不改写：保留原运行名称、首步已采集输入和末步实际结果，明确“未记录预期”，不推断验证通过。

`trace finish` 可以增加 verification；旧调用省略时仍能保存旧格式：

```json
{
  "runId":"本次真实运行编号",
  "status":"completed",
  "summary":"隔离测试，实际调用价格上限函数。",
  "verification":{
    "goal":"价格超过上限时返回上限",
    "inputs":[{"name":"价格","value":"150"}],
    "expected":"120",
    "actual":"120",
    "keyValues":[{"name":"本次上限","value":"120"}],
    "verdict":"matched"
  }
}
```

上例仅说明格式，不是一次真实运行。调用者应在执行前确定预期，实际调用后填写返回值和实际采集的变量，再用 matched（符合预期）、different（不同）、notCompared（未比较）明确比较结果。执行结束不等于符合预期；failed/cancelled 不能填写 matched。阅读器不独立执行程序或证明生产者未伪造数据。

goal/expected/actual 各最多2000字符且非空，inputs最多10项，keyValues最多5项，名称最多100字符、值最多2000字符，同组名称不得重复。摘要与当前运行原子完成，已结束记录不能补写或覆盖。保存仍为 schemaVersion:1，增加可选字段；新版读取旧记录不要求迁移，旧版软件可能不理解新增字段，CLI和桌面应配套更新。

Python SDK：

```python
from codetrail_trace import verification
expected = 120  # 在执行之前确定
source = location(project_root, final_price)
actual = final_price(150, 120)  # 本次真实调用
recorder.step("计算价格", f"实际返回 {actual}", "按本次返回值记录。",
              source=source, inputs={"价格":150}, outputs={"结果":actual})
recorder.finish("completed", "实际执行一次隔离函数。", verification=verification(
    "价格超过上限时返回上限", inputs={"价格":150}, expected=str(expected),
    actual=str(actual), key_values={"本次上限":120},
    verdict="matched" if actual == expected else "different"))
```

示例代码需由实际程序执行才构成观测。敏感数据仍须显式允许清单；SDK的已知敏感字段名处理不是通用脱敏保证，避免向摘要文本写入密钥、用户完整历史或原始请求。

`agent validate` 的 record 选项可自动记录实际检查命令，详见交付协议。默认只保存程序名称、退出码、耗时和计数，不复制命令参数/原始输出。自动中间值属于“检查结果计数”，不能当作业务变量。超过50次/2 MiB等原有限制时明确报错，不自动删除旧记录。
