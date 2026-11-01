"""Independent objective checks, materialized outside the agent workspace after exit."""
import pathlib
import runpy
import sys
workspace = pathlib.Path(sys.argv[1])
app = runpy.run_path(str(workspace / "app.py"))
assert app["slug"]("\tHello\nWorld\t") == "hello-world"
assert app["slug"]("  ") == ""
assert app["slug"]("A_B C!") == "a_b-c!"
assert app["slug"]("界 X") == "界-x"
