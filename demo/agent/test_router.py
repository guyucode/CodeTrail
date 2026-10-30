"""阅读器的演示代码。测试关联只是证据链接，不代表已运行或已覆盖。"""
from agent.model_router import choose_route, build_context


def test_complex_tasks_use_cloud():
    assert choose_route(0.9).primary == "cloud-large"


def test_system_message_is_preserved():
    messages = [{"role": "system", "content": "Be precise"}]
    messages += [{"role": "user", "content": str(i)} for i in range(20)]
    context = build_context(messages, limit=3)
    assert context[0]["role"] == "system"
    assert len(context) == 4

