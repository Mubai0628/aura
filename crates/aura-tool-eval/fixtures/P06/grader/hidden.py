import unittest
from graph import find_cycle
from model import CycleError, MissingDependency, Task
from planner import plan, waves


class Cycles(unittest.TestCase):
    def test_self_loop_has_a_closed_witness(self):
        self.assertEqual(find_cycle({"a": ("a",)}), ("a", "a"))
        with self.assertRaises(CycleError) as caught:
            plan([Task("a", ("a",))])
        self.assertEqual(caught.exception.cycle, ("a", "a"))

    def test_three_node_cycle_is_rejected_before_scheduling(self):
        tasks = [Task("c", ("a",)), Task("b", ("c",)), Task("a", ("b",))]
        with self.assertRaises(CycleError) as caught:
            plan(tasks)
        self.assertEqual(caught.exception.cycle, ("a", "b", "c", "a"))
        self.assertEqual(str(caught.exception), "Dependency cycle: a -> b -> c -> a")

    def test_waves_cannot_turn_a_cycle_into_a_successful_plan(self):
        with self.assertRaises(CycleError):
            waves([Task("a", ("b",)), Task("b", ("a",))])

    def test_disconnected_cycles_choose_the_lexical_first_witness(self):
        graph = {"z": ("y",), "y": ("z",), "b": ("a",), "a": ("b",)}
        self.assertEqual(find_cycle(graph), ("a", "b", "a"))
        self.assertEqual(find_cycle(dict(reversed(list(graph.items())))), ("a", "b", "a"))

    def test_a_prefix_outside_the_cycle_is_not_part_of_the_witness(self):
        graph = {"0-start": ("a",), "a": ("b",), "b": ("a",)}
        self.assertEqual(find_cycle(graph), ("a", "b", "a"))

    def test_diamond_sharing_does_not_report_a_cycle(self):
        tasks = [Task("d", ("b", "c")), Task("c", ("a",)), Task("a"), Task("b", ("a",))]
        self.assertEqual([item.name for item in plan(tasks)], ["a", "b", "c", "d"])

    def test_missing_dependency_validation_precedes_cycle_analysis(self):
        with self.assertRaises(MissingDependency):
            plan([Task("a", ("a",)), Task("z", ("missing",))])

    def test_multiple_edges_and_unicode_labels_are_stable(self):
        self.assertEqual(find_cycle({"甲": ("乙", "乙"), "乙": ("甲",)}), ("乙", "甲", "乙"))


if __name__ == "__main__":
    unittest.main()
