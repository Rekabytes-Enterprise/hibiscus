import unittest
from app import slug
class Visible(unittest.TestCase):
    def test_simple(self): self.assertEqual(slug("Hello World"), "hello-world")
    def test_spaces(self): self.assertEqual(slug("  Hello   World "), "hello-world")
