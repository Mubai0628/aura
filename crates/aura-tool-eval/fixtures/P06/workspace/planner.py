"""Turn validated task declarations into deterministic schedules."""
from dataclasses import dataclass
from typing import Iterable
from graph import ordered
from model import Task, normalize


@dataclass(frozen=True)
class Scheduled:
    name: str
    start: int
    end: int
    dependencies: tuple[str, ...]


def plan(tasks: Iterable[Task]) -> list[Scheduled]:
    indexed = normalize(tasks)
    adjacency = {name: task.dependencies for name, task in indexed.items()}
    names = ordered(adjacency)
    ends: dict[str, int] = {}
    result: list[Scheduled] = []
    for name in names:
        task = indexed[name]
        start = max((ends.get(dependency, 0) for dependency in task.dependencies), default=0)
        end = start + task.duration
        result.append(Scheduled(name, start, end, task.dependencies))
        ends[name] = end
    return result


def total_duration(schedule: Iterable[Scheduled]) -> int:
    return max((item.end for item in schedule), default=0)


def waves(tasks: Iterable[Task]) -> list[list[str]]:
    schedule = plan(tasks)
    by_start: dict[int, list[str]] = {}
    for item in schedule:
        by_start.setdefault(item.start, []).append(item.name)
    return [sorted(by_start[start]) for start in sorted(by_start)]


def ready_after(tasks: Iterable[Task], completed: Iterable[str]) -> list[str]:
    indexed = normalize(tasks)
    done = set(completed)
    return [name for name, task in indexed.items() if name not in done and set(task.dependencies) <= done]
