import unittest
from graph import edges, find_cycle, ordered


class GraphTests(unittest.TestCase):
    def test_no_cycle_in_a_chain(self):
        self.assertEqual(find_cycle({"a": (), "b": ("a",), "c": ("b",)}), ())

    def test_shared_dependency_is_not_a_cycle(self):
        graph = {"a": (), "b": ("a",), "c": ("a",), "d": ("b", "c")}
        self.assertEqual(find_cycle(graph), ())
        self.assertEqual(ordered(graph), ["a", "b", "c", "d"])

    def test_edges_are_stable_and_unique(self):
        graph = {"z": ("b", "a", "a"), "a": (), "b": ()}
        self.assertEqual(edges(graph), [("z", "a"), ("z", "b")])

    def test_newly_ready_nodes_enter_lexical_queue(self):
        self.assertEqual(ordered({"z": (), "b": ("a",), "a": ()}), ["a", "b", "z"])


if __name__ == "__main__":
    unittest.main()
