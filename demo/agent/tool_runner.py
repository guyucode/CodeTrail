"""工具执行入口：校验请求，在执行前确认工具是否已授权。"""
from dataclasses import dataclass


@dataclass
class ToolRequest:
    name: str
    arguments: dict
    approved: bool = False


def validate_request(request: ToolRequest, registry: dict) -> None:
    """拒绝未知工具和未确认请求，不默默忽略错误。"""
    if request.name not in registry:
        raise ValueError(f"unknown tool: {request.name}")
    if not request.approved:
        raise PermissionError("tool request needs approval")


def execute_tool(request: ToolRequest, registry: dict) -> dict:
    """把结果交回调用方；异常由上层保留并展示。"""
    validate_request(request, registry)
    result = registry[request.name](**request.arguments)
    return {"tool": request.name, "result": result, "status": "ok"}

