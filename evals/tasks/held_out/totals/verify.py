"""Independent objective checks, materialized outside the agent workspace after exit."""
import pathlib
import runpy
import sys
workspace = pathlib.Path(sys.argv[1])
app = runpy.run_path(str(workspace / "app.py"))
assert app["totals"]([]) == {}
assert app["totals"]([("b", 5), ("a", -1), ("b", -7)]) == {"b": -2, "a": -1}
assert list(app["totals"]([("b", 0), ("a", 2), ("b", 4)])) == ["b", "a"]
assert app["totals"](iter([("a", 1), ("a", 2)])) == {"a": 3}
