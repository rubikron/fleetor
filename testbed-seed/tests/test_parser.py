import unittest

from src.parser import ParseError, Record, parse_line, parse_lines


class ParseLineTests(unittest.TestCase):
    def test_parses_a_well_formed_line(self):
        record = parse_line("10.0.0.1 GET /health 200 12")
        self.assertEqual(
            record, Record(ip="10.0.0.1", method="GET", path="/health", status=200, duration_ms=12)
        )

    def test_skips_blank_and_comment_lines(self):
        self.assertIsNone(parse_line(""))
        self.assertIsNone(parse_line("   \n"))
        self.assertIsNone(parse_line("# a comment"))

    def test_rejects_wrong_field_count(self):
        with self.assertRaises(ParseError):
            parse_line("10.0.0.1 GET /health 200")

    def test_rejects_non_numeric_status(self):
        with self.assertRaises(ParseError):
            parse_line("10.0.0.1 GET /health OK 12")

    def test_parse_lines_filters_blanks(self):
        lines = ["# header", "", "10.0.0.1 GET /a 200 5", "10.0.0.2 GET /b 404 9"]
        self.assertEqual(len(list(parse_lines(lines))), 2)


if __name__ == "__main__":
    unittest.main()
