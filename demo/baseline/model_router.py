"""模型路由示例：按任务复杂度选择模型，并保留降级路径。"""
from dataclasses import dataclass
from typing import Iterator


@dataclass(frozen=True)
class Route:
    primary: str
    fallback: str
    max_attempts: int = 2


def choose_route(complexity: float, prefer_local: bool = True) -> Route:
    """简单任务优先本地；复杂任务交给云端模型。"""
    if prefer_local and complexity < 0.5:
        return Route("local-small", "cloud-large")
    return Route("cloud-large", "local-small")


def stream_with_fallback(route: Route, providers: dict, prompt: str) -> Iterator[str]:
    """调用主模型，失败后切换备用模型。"""
    for model in (route.primary, route.fallback):
        try:
            yield from providers[model].stream(prompt)
            return
        except (TimeoutError, ConnectionError):
            if model == route.fallback:
                raise


def build_context(messages: list[dict], limit: int = 12) -> list[dict]:
    """限制历史长度；始终保留首条系统约束。"""
    if not messages:
        return []
    system = [m for m in messages if m.get("role") == "system"][:1]
    history = [m for m in messages if m.get("role") != "system"]
    return system + history[-limit:]

