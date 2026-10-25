# Build planner

Run `python -m unittest discover -p "test*.py"`; no third-party modules are needed.
The CLI accepts JSON task declarations on stdin. Tasks name their prerequisites,
and a task starts when all its prerequisites have finished. Output order is stable.
