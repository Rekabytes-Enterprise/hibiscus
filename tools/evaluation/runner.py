"""One execution path for paired offline and explicitly configured live runs."""
from contextlib import nullcontext
import hashlib
import json
import os
from pathlib import Path
import platform
import sys
import tempfile
import time

from .execution import environment, grade, prepare, process, session_evidence
from .fixture import Fixture
from .spec import InvalidSpec, digest, load_plan, load_suite, read_file


def config(plan, base_url, key_env, preflight=False):
    provider = plan.provider
    return {"model": f"{provider['id']}/{provider['model']['id']}",
            "maxSteps": plan.limits["maxSteps"], "providers": [{
                "id": provider["id"], "baseUrl": base_url,
                "api": "responses" if preflight else provider["api"],
                "apiKeyEnv": key_env, "models": [provider["model"]]}]}


def inspect_variant(plan, variant, binary, root):
    """Ask the native registry, rather than duplicating its schema/catalog rules."""
    batches = (json.dumps([{"name": "list_extensions", "arguments": {}}]),)
    prompt = "Inspect installed extension metadata only. Do not execute extension programs."
    with Fixture(batches) as fixture:
        cfg = config(plan, fixture.base_url, "HIBISCUS_EVAL_FIXTURE_KEY", preflight=True)
        prepare(root, (), variant, cfg)
        result = process([str(binary), prompt], root / "workspace",
                         environment(root, ("HIBISCUS_EVAL_FIXTURE_KEY", "SYNTHETIC")),
                         plan.limits["timeoutSeconds"], root / "native")
    evidence = session_evidence(root / "app", [plan.provider["id"], plan.provider["model"]["id"]], prompt)
    report = evidence["extensionReport"]
    catalog = report.get("catalog") if isinstance(report, dict) else None
    errors = []
    if result["error"] or not evidence["valid"] or fixture.errors or not isinstance(catalog, dict):
        errors.append("native_preflight_failed")
    elif (catalog.get("errors") != []
          or {e["id"] for e in catalog.get("extensions", [])} != {name for name, _ in variant.extensions}
          or any(t["isError"] for t in evidence["tools"])):
        errors.append("native_extension_validation_failed")
    definitions = fixture.requests[0].get("tools", []) if fixture.requests else []
    if not definitions:
        errors.append("missing_advertised_tool_catalog")
    return {"valid": not errors, "errors": errors, "toolCatalogDigest": digest(definitions),
            "toolCount": len(definitions), "instructionDigest": hashlib.sha256(variant.instructions).hexdigest(),
            "contentDigest": variant.identity,
            "extensions": [{"id": name, "contentDigest": digest([f.identity() for f in images])}
                           for name, images in variant.extensions]}


def run_case(plan, task, variant, binary, root, profile="repair"):
    fixture_context = Fixture(task.offline, profile) if plan.mode == "offline" else nullcontext(None)
    with fixture_context as fixture:
        key_env = "HIBISCUS_EVAL_FIXTURE_KEY" if fixture else plan.provider["apiKeyEnv"]
        key = "SYNTHETIC" if fixture else os.environ[key_env]
        base_url = fixture.base_url if fixture else plan.provider["baseUrl"]
        prepare(root, task.files, variant, config(plan, base_url, key_env))
        # The key is supplied only to the agent. Independent graders never receive it.
        execution = process([str(binary), task.prompt], root / "workspace",
                            environment(root, (key_env, key)), plan.limits["timeoutSeconds"], root / "native")
    evidence = session_evidence(root / "app", [plan.provider["id"], plan.provider["model"]["id"]], task.prompt)
    objective = grade(task, root, environment(root), plan.limits["checkTimeoutSeconds"])
    objective["successfulToolObserved"] = any(not t["isError"] for t in evidence["tools"])
    errors = list(evidence["errors"])
    if fixture and fixture.errors:
        errors.extend(fixture.errors)
    solved = (execution["error"] is None and not errors
              and all(objective[k] for k in ("checksPassed", "requiredFilesChanged",
                                            "preservedFilesUnchanged", "successfulToolObserved")))
    return {"taskId": task.id, "taskDigest": task.identity, "split": task.split,
            "variantId": variant.id, "configurationDigest": digest([plan.identity, variant.identity]),
            "execution": execution, "structure": {"valid": not errors, "errors": errors},
            "objective": objective, "solved": solved, "tokens": evidence["tokens"],
            "costUsd": None, "tools": evidence["tools"], "sessionDigest": evidence["sessionDigest"],
            "providerRequests": len(fixture.requests) if fixture else None,
            "fixtureProfile": profile if fixture else None}


def summarize(results, variants):
    summaries = []
    baseline = {(r["taskId"], r["repetition"]): r for r in results if r["variantId"] == "baseline"}
    for variant in variants:
        runs = [r for r in results if r["variantId"] == variant.id]
        solved = sum(r["solved"] for r in runs)
        item = {"variantId": variant.id, "runs": len(runs), "solved": solved,
                "solveRate": solved / len(runs) if runs else None,
                "executionErrors": sum(r["execution"]["error"] is not None for r in runs),
                "structurallyInvalid": sum(not r["structure"]["valid"] for r in runs),
                "toolErrors": sum(t["isError"] for r in runs for t in r["tools"]),
                "durationMs": sum(r["execution"]["durationMs"] for r in runs),
                "tokens": {}, "costUsd": None}
        for key in ("input", "output", "cacheRead"):
            values = [r["tokens"].get(key) if r["tokens"] else None for r in runs]
            item["tokens"][key] = sum(values) if values and all(v is not None for v in values) else None
        if variant.id != "baseline":
            pairs = [(r["solved"], baseline[(r["taskId"], r["repetition"])]["solved"]) for r in runs]
            item["versusBaseline"] = {"wins": sum(a and not b for a, b in pairs),
                                      "losses": sum(b and not a for a, b in pairs),
                                      "ties": sum(a == b for a, b in pairs)}
        summaries.append(item)
    return summaries


def evaluate(plan_path, binary, split="selection", live=False):
    plan = load_plan(Path(plan_path), live)
    suite = load_suite(plan.suite)
    tasks = [t for t in suite.tasks if t.split == split]
    if not tasks:
        raise InvalidSpec("Selected split has no tasks")
    binary = Path(binary).resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise InvalidSpec("Pass an executable Hibiscus binary")
    binary_bytes = read_file(binary, 512 * 1024 * 1024)
    binary_digest = hashlib.sha256(binary_bytes).hexdigest()
    if live and not os.environ.get(plan.provider["apiKeyEnv"], "").strip():
        raise InvalidSpec("Configured live-provider API-key environment variable is missing")
    started = time.time()
    results = []
    initials = []
    with tempfile.TemporaryDirectory(prefix="hibiscus-eval-") as directory:
        root = Path(directory)
        # Every run uses one frozen executable, even if the source build changes.
        binary = root / "hibiscus"
        binary.write_bytes(binary_bytes)
        binary.chmod(0o700)
        structures = [inspect_variant(plan, v, binary, root / f"preflight-{i}")
                      for i, v in enumerate(plan.variants)]
        comparable = (all(s["valid"] for s in structures)
                      and len({s["toolCatalogDigest"] for s in structures}) == 1)
        if comparable:
            # A starter that already passes is a broken benchmark, not an easy win.
            for index, task in enumerate(tasks):
                initial = root / f"initial-{index}"
                prepare(initial, task.files, plan.variants[0], {})
                outcome = grade(task, initial, environment(initial), plan.limits["checkTimeoutSeconds"])
                initials.append({"taskId": task.id, "checksPassed": outcome["checksPassed"],
                                 "checkExecution": outcome["checkExecution"]})
                if outcome["checksPassed"] or outcome["checkExecution"]["error"] != "nonzero_exit":
                    raise InvalidSpec("Task starter must fail objective checks without a grader execution error")
            # Alternate paired order to avoid always giving one variant the first slot.
            for repetition in range(plan.repetitions):
                order = plan.variants if repetition % 2 == 0 else tuple(reversed(plan.variants))
                for task in tasks:
                    for variant in order:
                        result = run_case(plan, task, variant, binary, root / f"run-{len(results)}")
                        result["repetition"] = repetition + 1
                        results.append(result)
    return {"formatVersion": 1, "mode": plan.mode, "planId": plan.id,
            "planDigest": plan.identity, "suiteId": suite.id, "suiteDigest": suite.identity,
            "split": split, "repetitions": plan.repetitions,
            "startedAtUnix": int(started), "durationMs": round((time.time() - started) * 1000),
            "binaryDigest": binary_digest,
            "environment": {"system": platform.system(), "architecture": platform.machine(),
                            "python": platform.python_version(),
                            "pythonDigest": hashlib.sha256(Path(sys.executable).read_bytes()).hexdigest(),
                            "shellDigest": hashlib.sha256(Path("/bin/sh").read_bytes()).hexdigest()},
            "controls": {"provider": plan.provider, "thinking": "off", "limits": plan.limits,
                         "toolCatalogDigest": structures[0]["toolCatalogDigest"]},
            "structure": {"valid": all(s["valid"] for s in structures),
                          "comparable": comparable,
                          "variants": [{"id": v.id, **s} for v, s in zip(plan.variants, structures)]},
            "initial": initials, "results": results, "summary": summarize(results, plan.variants),
            "limitations": ["Offline fixtures measure harness execution, not model quality.",
                            "Held-out graders are withheld during execution, not sandboxed or secret.",
                            "Fresh workspaces and homes are isolation, not a filesystem/network sandbox.",
                            "Cost is unknown. Native maxSteps/output limits and wall time are not a total token or spend cap.",
                            "Repetitions use independent sessions; remote provider versions and sampling are not pinned.",
                            "Reports omit transcripts; agent or extension code may still access inherited OS permissions."]}


def write_report(report, path):
    # Never overwrite a previous experiment or write keys/transcripts to a report.
    path = Path(path)
    content = (json.dumps(report, indent=2, sort_keys=True) + "\n").encode()
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
