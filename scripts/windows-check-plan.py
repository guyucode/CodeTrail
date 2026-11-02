"""Construct an explicit reviewed check plan outside the indexed repository."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BASE = '36183a3f8fb44e16d94fd3b434ff1f6997fa057a'
REASONS = {'docs/MAC_RELEASE.md': 'Mac 0.3发布与数据保留说明，不改变Windows运行行为；保留实际平台验收边界。','Cargo.lock': '锁定第二版共享网络、网卡枚举和持久化依赖；生成依赖图不属于业务源码片段。',
 'Cargo.toml': '在现有 workspace 中增加协作库，原桌面 crate 隔离方式保持不变。',
 'crates/reader-cli/Cargo.toml': '将 CLI 包版本与第二版统一为0.3；命令和旧协议保持兼容。',
 'crates/reader-collab/Cargo.toml': '声明局域网协作库及其显式依赖；具体行为已绑定到主持、权限和分工实现。',
 'crates/reader-collab/src/client.rs': '客户端导入、URL解析辅助及模块声明是共用支撑；IP限制和实际请求函数已有职责关联。',
 'crates/reader-collab/src/lib.rs': '协议基础类型和摘要/随机凭证工具在多个协作功能间共享；Version、Manifest、CommentInput、TaskInput、Scope已分别关联，余下声明不假装属于单一业务分支。',
 'crates/reader-collab/src/server.rs': '服务导入、生命周期结构和通用路由配置支撑主持入口；网卡识别、Host启动、来源检查和真实地址回归已关联。',
 'crates/reader-collab/src/service.rs': '私有存储结构、字段/凭证校验辅助和模块导入共用于所有会话操作；分派、公开视图、版本结构校验和原子持久化已关联。',
 'crates/reader-collab/src/tasks.rs': '作用域与重叠辅助、模块导入和嵌套图遍历支撑有明确职责的耦合、依赖、导出函数；不把代码模板行伪装成独立功能。',
 'crates/reader-collab/tests/collaboration.rs': '合成会话/成员/任务夹具与多条回归共享，关键权限、版本、并发、分工、合并及真实网卡联机用例均有对应功能证据。',
 'crates/reader-core/Cargo.toml': '包版本0.3及本地协作库依赖声明，与已有核心实现共享。',
 'crates/reader-core/src/collaboration.rs': '共读Git构造器改用统一后台启动，实际Windows标志已关联项目加载的background约束；不改变共读命令参数和版本语义。',
 'crates/reader-core/src/lib.rs': '注册私有process模块，公开API和扫描边界不变。',
 'crates/reader-core/src/relations.rs': '新增正向功能调用线索已关联Python关系与耦合分工；函数周围的声明空白不是另一个业务实现。',
 'crates/reader-core/tests/collaboration.rs': '隔离Git项目夹具、换行及未提交代码回归辅助多项测试；真实双客户端与正向Python用例已关联，不触碰真实人工记录。',
 'package-lock.json': '仅同步根包版本，不手工改动依赖解析结果。',
 'package.json': '第二版版本号和浏览器协作交互验收命令；真实测试实现已关联且明确为传输替身。',
 'scripts/package-windows.ps1': '以实际配置版本命名安装器与工具包，替换旧版硬编码字符串；属于既有发布脚本，不代表本次已做Windows验收。',
 'scripts/test-windows-install.ps1': '从实际配置版本定位待测安装器，替换旧版文件名；原安装与原生验收脚本证据保留。',
 'src-tauri/Cargo.lock': '锁定桌面新增协作依赖及0.3包版本；依赖图由Cargo生成。',
 'src-tauri/Cargo.toml': '同步桌面包版本0.3，业务依赖继续由reader-core提供。',
 'src-tauri/src/lib.rs': '新增主持生命周期管理与命令注册为桌面集成胶水；实际共读和另存为入口已关联，旧命令仍保持兼容。',
 'src-tauri/tauri.conf.json': '版本0.3及macOS本地网络用途Info.plist接入；不改变原CSP或扩大源码访问权限。',
 'src/App.tsx': '项目切换时关闭旧共读窗口的状态清理及替换旧工作面板调用的删除侧；共读入口和现有阅读导航已关联。',
 'src/components/CollaborationPanel.tsx': '顶层导入、类型与弹窗通用辅助由批注、主持和任务窗口共用；主组件、草稿保存及真实交互验收已关联。',
 'src/components/WorkPanel.tsx': '仅新增协作组件及类型导入，并替换原面板参数；个人批注和动态原功能证据保持，删除侧属于兼容接入。',
 'src/styles.css': '追加共读面板与任务整理窗口样式，复用现有布局，不属于代码关系或权限判断。',
 'tests/collaboration.test.ts': '隔离SSR验收检查演示不自动共享、个人入口保留；真实交互与网络另有专门测试，不将界面静态渲染当作联网成功。',
 'crates/reader-core/src/git.rs': 'Git共享构造器、树对象大小字段、生命周期回收与导入支撑批量读取；新读取入口及连续复用测试已关联，删除侧为替换旧逐文件两次启动。',
 'crates/reader-core/src/process.rs': '文件级注释和标准库导入，实际Windows隐藏标志已关联background函数。',
 'crates/reader-core/src/semantic.rs': 'Node版本探测复用与Pyright启动相同的后台构造器；现有Pyright入口证据与新增background约束覆盖实际策略，删除侧只替换构造调用。',
 'scripts/windows-check-plan.py': '更新Windows第二版发布基线与逐路径覆盖说明；实际测试命令保持完整，遇到未审阅路径仍拒绝发布。',
 'README.md': '更新Windows 0.3预览版下载入口，Mac下载仍指向已发布0.2；不声称两台物理设备或Windows 10/11人工验收通过。',
 'docs/WINDOWS_RELEASE.md': '同步0.3安装包和工具包文件名，明确局域网及后台读取改进和验证边界。'}

def main():
    cli = ROOT / 'target/debug' / ('reader-cli.exe' if os.name == 'nt' else 'reader-cli')
    report = json.loads(subprocess.check_output([str(cli), 'agent', 'coverage', str(ROOT), BASE], encoding='utf-8'))
    exceptions = []
    for file in report['files']:
        if file['needsExplanation']:
            if file['path'] not in REASONS:
                raise SystemExit('Unreviewed coverage exception: ' + file['path'])
            exceptions.append({'path': file['path'], 'changeDigest': file['changeDigest'], 'reason': REASONS[file['path']]})
    npm = 'npm.cmd' if os.name == 'nt' else 'npm'
    checks = [
        ('cargo', ['test', '--workspace', '--locked']),
        ('cargo', ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']),
        (npm, ['test']), (npm, ['run', 'build']),
        ('cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--locked']),
        (sys.executable, ['-B', '-m', 'unittest', 'discover', '-s', 'sdk/python', '-p', 'test_*.py']),
        ('git', ['diff', '--check', BASE]),
    ]
    plan = {'schemaVersion': 1, 'base': BASE, 'unassociated': exceptions,
            'checks': [{'program': program, 'args': args} for program, args in checks]}
    output = Path(os.environ.get('RUNNER_TEMP', tempfile.gettempdir())) / 'codetrail-windows-check-plan.json'
    output.write_text(json.dumps(plan, ensure_ascii=False), encoding='utf-8')
    print(output)

if __name__ == '__main__':
    main()
