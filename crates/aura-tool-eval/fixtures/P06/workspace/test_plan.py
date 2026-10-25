import json
import unittest
from model import MissingDependency, PlanError, Task, normalize
from planner import plan, ready_after, total_duration, waves
from render import as_json, as_text


class PlanTests(unittest.TestCase):
    def test_chain_runs_dependencies_first(self):
        tasks = [Task("ship", ("test",)), Task("test", ("build",)), Task("build", duration=3)]
        schedule = plan(tasks)
        self.assertEqual([item.name for item in schedule], ["build", "test", "ship"])
        self.assertEqual([(item.start, item.end) for item in schedule], [(0, 3), (3, 4), (4, 5)])

    def test_independent_tasks_use_lexical_order(self):
        self.assertEqual([item.name for item in plan([Task("z"), Task("a")])], ["a", "z"])

    def test_empty_plan_has_zero_duration(self):
        self.assertEqual(plan([]), [])
        self.assertEqual(total_duration([]), 0)
        self.assertEqual(waves([]), [])

    def test_duplicate_dependencies_are_one_edge(self):
        tasks = [Task("a"), Task("b", ("a", "a"))]
        self.assertEqual(plan(tasks)[1].dependencies, ("a",))

    def test_missing_dependency_has_structured_details(self):
        with self.assertRaises(MissingDependency) as caught:
            plan([Task("build", ("tools",))])
        self.assertEqual(caught.exception.task, "build")
        self.assertEqual(caught.exception.dependency, "tools")

    def test_duplicate_names_are_rejected(self):
        with self.assertRaises(PlanError):
            normalize([Task("a"), Task("a")])

    def test_durations_are_positive_integers(self):
        for invalid in [0, -1, 0.5, True]:
            with self.assertRaises(PlanError):
                Task("a", duration=invalid)

    def test_ready_after_returns_only_unfinished_ready_tasks(self):
        tasks = [Task("a"), Task("b", ("a",)), Task("c", ("b",))]
        self.assertEqual(ready_after(tasks, ["a"]), ["b"])

    def test_waves_group_tasks_by_start_time(self):
        self.assertEqual(waves([Task("a"), Task("b"), Task("c", ("a",))]), [["a", "b"], ["c"]])

    def test_renderers_keep_schedule_information(self):
        schedule = plan([Task("a")])
        self.assertEqual(json.loads(as_json(schedule))["duration"], 1)
        self.assertEqual(as_text(schedule), "a: 0..1\nduration: 1")

    def test_join_waits_for_the_longest_prerequisite(self):
        schedule = plan([
            Task("join", ("fast", "slow"), duration=2),
            Task("fast", duration=1),
            Task("slow", duration=5),
        ])
        joined = next(item for item in schedule if item.name == "join")
        self.assertEqual((joined.start, joined.end), (5, 7))
        self.assertEqual(total_duration(schedule), 7)

    def test_unicode_names_survive_json(self):
        schedule = plan([Task("构建"), Task("发布", ("构建",))])
        document = json.loads(as_json(schedule))
        self.assertEqual([row["name"] for row in document["tasks"]], ["构建", "发布"])
        self.assertIn("构建", as_json(schedule))

    def test_the_input_sequence_is_not_mutated(self):
        tasks = [Task("z"), Task("a")]
        before = list(tasks)
        plan(tasks)
        self.assertEqual(tasks, before)

    def test_empty_names_are_rejected_at_the_boundary(self):
        for invalid in ["", " ", None]:
            with self.assertRaises(PlanError):
                Task(invalid)


if __name__ == "__main__":
    unittest.main()
