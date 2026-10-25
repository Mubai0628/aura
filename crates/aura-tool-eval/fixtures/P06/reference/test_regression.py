import unittest
from graph import find_cycle
from model import CycleError, Task
from planner import plan


class CycleRegression(unittest.TestCase):
    def test_graph_inspection_finds_the_original_back_edge(self):
        self.assertEqual(find_cycle({"a": ("b",), "b": ("a",)}), ("a", "b", "a"))

    def test_cyclic_builds_cannot_be_scheduled(self):
        with self.assertRaises(CycleError) as caught:
            plan([Task("a", ("b",)), Task("b", ("a",))])
        self.assertEqual(caught.exception.cycle, ("a", "b", "a"))
