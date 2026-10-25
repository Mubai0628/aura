import unittest
from search import find_positions
class Behavior(unittest.TestCase):
    def test_multibyte(self): self.assertEqual(find_positions('甲a甲', '甲'), [0, 2])
    def test_emoji(self): self.assertEqual(find_positions('🙂x🙂', '🙂'), [0, 2])
    def test_mixed(self): self.assertEqual(find_positions('Straße', 'ß'), [4])
    def test_non_overlapping(self): self.assertEqual(find_positions('aaa', 'aa'), [0])
    def test_empty(self): self.assertEqual(find_positions('x', ''), [])
    def test_no_match(self): self.assertEqual(find_positions('x', 'y'), [])
if __name__ == '__main__': unittest.main()
