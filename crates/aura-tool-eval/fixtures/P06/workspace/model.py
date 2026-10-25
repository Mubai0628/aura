"""Immutable task data and user-facing validation errors."""
from dataclasses import dataclass
from typing import Iterable


class PlanError(ValueError):
    """The input cannot be converted into a usable plan."""


class MissingDependency(PlanError):
    def __init__(self, task: str, dependency: str):
        self.task = task
        self.dependency = dependency
        super().__init__(f"{task} depends on missing task {dependency}")


class CycleError(PlanError):
    def __init__(self, cycle: tuple[str, ...]):
        self.cycle = cycle
        super().__init__("Dependency cycle: " + " -> ".join(cycle))


@dataclass(frozen=True)
class Task:
    name: str
    dependencies: tuple[str, ...] = ()
    duration: int = 1

    def __post_init__(self):
        if not isinstance(self.name, str) or not self.name.strip():
            raise PlanError("task names cannot be empty")
        if type(self.duration) is not int or self.duration < 1:
            raise PlanError("task durations must be positive integers")
        if any(not isinstance(name, str) or not name.strip() for name in self.dependencies):
            raise PlanError("dependency names cannot be empty")


def normalize(tasks: Iterable[Task]) -> dict[str, Task]:
    result: dict[str, Task] = {}
    for task in tasks:
        if task.name in result:
            raise PlanError(f"duplicate task {task.name}")
        result[task.name] = Task(task.name, tuple(sorted(set(task.dependencies))), task.duration)
    result = dict(sorted(result.items()))
    for name, task in result.items():
        for dependency in task.dependencies:
            if dependency not in result:
                raise MissingDependency(name, dependency)
    return result
