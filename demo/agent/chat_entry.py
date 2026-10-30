"""关系导航示例：入口未修改，但其调用的路由和降级函数发生了变化。"""
from .model_router import choose_route as select_route
from .model_router import build_context, stream_with_fallback


def prepare_prompt(messages: list[dict]) -> str:
    """把裁剪后的消息转换为提示词。"""
    context = build_context(messages, limit=12)
    return "\n".join(str(message.get("content", "")) for message in context)


def stream_chat(messages: list[dict], complexity: float, providers: dict):
    """依次准备路由与提示词，再把流式结果交给调用方。"""
    route = select_route(complexity)
    prompt = prepare_prompt(messages)
    return stream_with_fallback(route, providers, prompt)


def batch_chat(requests: list[dict], providers: dict):
    """通过同一个入口处理批量请求。"""
    return [stream_chat(item["messages"], item["complexity"], providers) for item in requests]


def get_route_callback():
    """返回函数对象，不在这里执行路由；这是引用而不是调用。"""
    return select_route


def dispatch_plugin(handler, payload):
    """动态回调的具体目标取决于调用方，静态分析不能猜测。"""
    return handler(payload)
