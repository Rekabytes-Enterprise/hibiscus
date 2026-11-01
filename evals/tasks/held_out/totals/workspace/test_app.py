import unittest
from app import totals
class Visible(unittest.TestCase):
    def test_single(self): self.assertEqual(totals([("a", 2)]), {"a": 2})
    def test_repeat(self): self.assertEqual(totals([("a", 2), ("a", 3)]), {"a": 5})
