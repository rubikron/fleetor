import unittest

from src.parser import Record
from src.stats import aggregate, percentile


def rec(status=200, path="/a", duration=10):
    return Record(ip="10.0.0.1", method="GET", path=path, status=status, duration_ms=duration)


class PercentileTests(unittest.TestCase):
    def test_empty_input_is_zero(self):
        self.assertEqual(percentile([], 50), 0)

    def test_median_of_odd_length(self):
        self.assertEqual(percentile([1, 2, 3], 50), 2)

    def test_p100_is_the_max(self):
        self.assertEqual(percentile([5, 1, 9], 100), 9)


class AggregateTests(unittest.TestCase):
    def test_counts_total_and_statuses(self):
        agg = aggregate([rec(200), rec(200), rec(404)])
        self.assertEqual(agg.total, 3)
        self.assertEqual(agg.by_status, {200: 2, 404: 1})

    def test_ranks_paths_by_frequency(self):
        agg = aggregate([rec(path="/a"), rec(path="/b"), rec(path="/b")])
        self.assertEqual(list(agg.by_path)[0], "/b")

    def test_empty_input_produces_zeroes(self):
        agg = aggregate([])
        self.assertEqual(agg.total, 0)
        self.assertEqual(agg.p50_ms, 0)


if __name__ == "__main__":
    unittest.main()
