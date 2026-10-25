"""Read task declarations from stdin and print a plan without external services."""
import json
import sys
from model import PlanError, Task
from planner import plan
from render import as_json


def main() -> int:
    try:
        data = json.load(sys.stdin)
        tasks = [Task(item["name"], tuple(item.get("dependencies", [])), item.get("duration", 1)) for item in data]
        print(as_json(plan(tasks)))
        return 0
    except (PlanError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
