#!/usr/bin/env python3

import unittest

from cubicle import flatten, rel_time, si_bytes


class TestDev(unittest.TestCase):
    def test_rel_time(self):
        def d(duration):
            return rel_time(1, 1 + duration)

        self.assertEqual(rel_time(0), "N/A")
        self.assertEqual(rel_time(0, 100), "N/A")
        self.assertEqual(rel_time(60, 120), "1 minutes")

        self.assertEqual(d(0), "0 minutes")
        self.assertEqual(d(30), "0 minutes")
        self.assertEqual(d(31), "1 minutes")
        self.assertEqual(d(89), "1 minutes")
        self.assertEqual(d(90), "2 minutes")
        self.assertEqual(d(150), "2 minutes")
        self.assertEqual(d(151), "3 minutes")
        self.assertEqual(d(59 * 60 + 29), "59 minutes")
        self.assertEqual(d(59 * 60 + 30), "1 hours")
        self.assertEqual(d(23 * 60 * 60 + 29 * 60 + 59), "23 hours")
        self.assertEqual(d(23 * 60 * 60 + 30 * 60), "1 days")
        self.assertEqual(d(365 * 24 * 60 * 60), "365 days")

    def test_si_bytes(self):
        self.assertEqual(si_bytes(0), "0 B")
        self.assertEqual(si_bytes(999), "999 B")
        self.assertEqual(si_bytes(1_000), "1.0 kB")
        self.assertEqual(si_bytes(1_049), "1.0 kB")
        self.assertEqual(si_bytes(1_050), "1.1 kB")
        self.assertEqual(si_bytes(999_949), "999.9 kB")
        self.assertEqual(si_bytes(999_950), "1.0 MB")
        self.assertEqual(si_bytes(1_000_000), "1.0 MB")
        self.assertEqual(si_bytes(999_949_999), "999.9 MB")
        self.assertEqual(si_bytes(999_950_000), "1.0 GB")
        self.assertEqual(si_bytes(999_949_999_999), "999.9 GB")
        self.assertEqual(si_bytes(999_950_000_000), "1.0 TB")
        self.assertEqual(si_bytes(999_949_999_999_999), "999.9 TB")

    def test_flatten(self):
        self.assertEqual(flatten(1, 2, 3), [1, 2, 3])
        self.assertEqual(flatten(1, ("a", "b", "c"), 3), [1, "a", "b", "c", 3])
        self.assertEqual(flatten(1, ["a", "b", "c"], 3), [1, "a", "b", "c", 3])
        self.assertEqual(flatten(1, [("a", ["b"]), "c"], 3), [1, "a", "b", "c", 3])
        self.assertEqual(flatten([1, 2, 3]), [1, 2, 3])
        self.assertEqual(flatten([1, ("a", "b", "c"), 3]), [1, "a", "b", "c", 3])
        self.assertEqual(flatten([1, ["a", "b", "c"], 3]), [1, "a", "b", "c", 3])
        self.assertEqual(flatten([1, [("a", ["b"]), "c"], 3]), [1, "a", "b", "c", 3])
        self.assertEqual(flatten(1), [1])
        self.assertEqual(flatten(), [])


if __name__ == "__main__":
    unittest.main()
