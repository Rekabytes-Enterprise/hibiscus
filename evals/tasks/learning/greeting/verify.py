"""Independent objective checks, materialized outside the agent workspace after exit."""
import pathlib
import runpy
import sys
workspace = pathlib.Path(sys.argv[1])
app = runpy.run_path(str(workspace / "app.py"))
assert app["greet"](" Ada ") == "Hello, Ada!"
assert app["greet"]("\t\n") == "Hello, friend!"
assert app["greet"]("界") == "Hello, 界!"
