"""Small formatters shared by the command-line preview and callers."""
import json
from typing import Iterable
from planner import Scheduled, total_duration


def as_rows(schedule: Iterable[Scheduled]) -> list[dict]:
    return [
        {
            "name": item.name,
            "start": item.start,
            "end": item.end,
            "dependencies": list(item.dependencies),
        }
        for item in schedule
    ]


def as_json(schedule: Iterable[Scheduled]) -> str:
    items = list(schedule)
    document = {"tasks": as_rows(items), "duration": total_duration(items)}
    return json.dumps(document, ensure_ascii=False, sort_keys=True)


def as_text(schedule: Iterable[Scheduled]) -> str:
    items = list(schedule)
    lines = [f"{item.name}: {item.start}..{item.end}" for item in items]
    lines.append(f"duration: {total_duration(items)}")
    return "\n".join(lines)
