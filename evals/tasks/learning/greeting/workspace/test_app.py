import unittest
from app import greet
class Visible(unittest.TestCase):
    def test_named(self): self.assertEqual(greet("Ada"), "Hello, Ada!")
    def test_blank(self): self.assertEqual(greet("  "), "Hello, friend!")
