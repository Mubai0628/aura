"""Stable graph operations; edges point from a task to its prerequisites."""
import heapq
from typing import Mapping


Graph = Mapping[str, tuple[str, ...]]


def find_cycle(graph: Graph) -> tuple[str, ...]:
    seen: set[str] = set()
    path: list[str] = []
    active: set[str] = set()

    def visit(name: str) -> tuple[str, ...]:
        if name in active:
            return tuple(path[path.index(name):] + [name])
        if name in seen:
            return ()
        seen.add(name)
        active.add(name)
        path.append(name)
        for dependency in sorted(set(graph[name])):
            cycle = visit(dependency)
            if cycle:
                return cycle
        path.pop()
        active.remove(name)
        return ()

    for name in sorted(graph):
        cycle = visit(name)
        if cycle:
            return cycle
    return ()


def ordered(graph: Graph) -> list[str]:
    remaining = {name: len(set(deps)) for name, deps in graph.items()}
    dependents: dict[str, list[str]] = {name: [] for name in graph}
    for name, dependencies in graph.items():
        for dependency in set(dependencies):
            dependents[dependency].append(name)
    ready = [name for name in graph if remaining[name] == 0]
    heapq.heapify(ready)
    result: list[str] = []
    while ready:
        name = heapq.heappop(ready)
        result.append(name)
        for dependent in sorted(dependents[name]):
            remaining[dependent] -= 1
            if remaining[dependent] == 0:
                heapq.heappush(ready, dependent)
    # The legacy caller still displays incomplete graphs as a best-effort list.
    result.extend(sorted(set(graph) - set(result)))
    return result


def edges(graph: Graph) -> list[tuple[str, str]]:
    return [(name, dependency) for name in sorted(graph) for dependency in sorted(set(graph[name]))]
