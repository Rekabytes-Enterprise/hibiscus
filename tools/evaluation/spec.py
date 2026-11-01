"""Versioned experiment inputs, validated once and frozen before execution."""
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat


class InvalidSpec(ValueError):
    pass


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=True).encode()).hexdigest()


def object_fields(value, required, optional=()):
    if not isinstance(value, dict) or not set(required) <= value.keys():
        raise InvalidSpec("Missing required object fields")
    if value.keys() - set(required) - set(optional):
        raise InvalidSpec("Unknown object fields")
    return value


def number(value, low, high):
    if type(value) is not int or not low <= value <= high:
        raise InvalidSpec(f"Expected integer in {low}..{high}")
    return value


def label(value):
    if not isinstance(value, str) or not re.fullmatch(r"[a-z][a-z0-9_-]{0,63}", value):
        raise InvalidSpec("Expected a short lowercase identifier")
    return value


def relative(value):
    if (not isinstance(value, str) or not value or len(value) > 256
            or any(ord(c) < 32 for c in value) or "\\" in value):
        raise InvalidSpec("Invalid relative file path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(p in ("", ".", "..", ".git") for p in value.split("/")):
        raise InvalidSpec("Paths must be normal relative paths outside Git metadata")
    return value


def read_file(path, maximum=1024 * 1024):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > maximum:
        raise InvalidSpec("Expected bounded regular input file, without symlinks")
    with path.open("rb") as stream:
        content = stream.read(maximum + 1)
    if len(content) > maximum:
        raise InvalidSpec("Input file grew beyond its byte limit")
    return content


def input_path(base, name):
    path = base
    for part in relative(name).split("/"):
        path = path / part
        if path.is_symlink():
            raise InvalidSpec("Input references cannot pass through symlinks")
    return path


def document(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise InvalidSpec("Duplicate JSON object key")
            result[key] = value
        return result
    try:
        return json.loads(read_file(path), object_pairs_hook=unique)
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        raise InvalidSpec("Invalid UTF-8 JSON input") from error


@dataclass(frozen=True)
class Image:
    path: str
    content: bytes
    mode: int

    def identity(self):
        return [self.path, self.mode, hashlib.sha256(self.content).hexdigest()]


def inventory(root):
    """No followed links, sockets or unbounded dependency/cache trees."""
    if root.is_symlink() or not root.is_dir():
        raise InvalidSpec("Expected an input directory without symlinks")
    files = []
    total = 0
    for index, path in enumerate(root.rglob("*")):
        if index >= 512:
            raise InvalidSpec("Evaluation directory exceeds 512 entries")
        name = relative(path.relative_to(root).as_posix())
        if path.is_symlink():
            raise InvalidSpec("Symbolic links are not evaluation inputs or outputs")
        if path.is_dir():
            continue
        content = read_file(path)
        total += len(content)
        files.append(Image(name, content, stat.S_IMODE(path.stat().st_mode)))
        if len(files) > 128 or total > 8 * 1024 * 1024:
            raise InvalidSpec("Evaluation file inventory exceeds 128 files or 8 MiB")
    return tuple(sorted(files, key=lambda f: f.path))


def materialize(images, directory):
    directory.mkdir(parents=True, mode=0o700, exist_ok=True)
    for image in images:
        path = directory / image.path
        path.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
        path.write_bytes(image.content)
        path.chmod(image.mode)


@dataclass(frozen=True)
class Task:
    id: str
    split: str
    prompt: str
    files: tuple
    grader: bytes
    changed: tuple
    preserved: tuple
    offline: tuple
    identity: str


@dataclass(frozen=True)
class Suite:
    id: str
    tasks: tuple
    identity: str


def load_suite(path):
    raw = object_fields(document(path), ["formatVersion", "id", "tasks"])
    number(raw["formatVersion"], 1, 1)
    label(raw["id"])
    if not isinstance(raw["tasks"], list) or not 1 <= len(raw["tasks"]) <= 32:
        raise InvalidSpec("Suite must contain 1..32 tasks")
    tasks = []
    for source in raw["tasks"]:
        task_path = input_path(path.parent, source)
        data = object_fields(document(task_path),
                             ["id", "split", "prompt", "changed", "preserved", "offline"])
        label(data["id"])
        if data["split"] not in ("learning", "selection", "held_out"):
            raise InvalidSpec("Unknown task split")
        prompt = data["prompt"]
        if (not isinstance(prompt, str) or not prompt.strip() or len(prompt) > 8192
                or prompt.startswith("-")):
            raise InvalidSpec("Invalid one-shot task prompt")
        files = inventory(task_path.parent / "workspace")
        paths = {f.path for f in files}
        if "AGENTS.md" in paths or any(f.path.startswith((".agents/", ".hibiscus/")) for f in files):
            raise InvalidSpec("Task files cannot override variant instructions or skills")
        for field in ("changed", "preserved"):
            if not isinstance(data[field], list) or not data[field]:
                raise InvalidSpec("Declare changed and preserved files for objective grading")
            for p in data[field]:
                relative(p)
                if p not in paths:
                    raise InvalidSpec("Scoring path missing from task workspace")
            if len(set(data[field])) != len(data[field]):
                raise InvalidSpec("Duplicate scoring file")
        if set(data["changed"]) & set(data["preserved"]):
            raise InvalidSpec("A path cannot be both changed and preserved")
        offline = data["offline"]
        if not isinstance(offline, list) or not 1 <= len(offline) <= 16:
            raise InvalidSpec("Expected 1..16 offline tool batches")
        # Offline calls are trusted benchmark code, never sent to a real provider.
        for batch in offline:
            if not isinstance(batch, list) or not 1 <= len(batch) <= 8:
                raise InvalidSpec("Invalid offline tool batch")
            for call in batch:
                object_fields(call, ["name", "arguments"])
                if not isinstance(call["name"], str) or not isinstance(call["arguments"], dict):
                    raise InvalidSpec("Invalid offline tool call")
        grader = read_file(task_path.parent / "verify.py", 65536)
        identity = digest([data, [f.identity() for f in files], hashlib.sha256(grader).hexdigest()])
        tasks.append(Task(data["id"], data["split"], prompt, files, grader,
                          tuple(data["changed"]), tuple(data["preserved"]),
                          tuple(json.dumps(b) for b in offline), identity))
    if len({t.id for t in tasks}) != len(tasks):
        raise InvalidSpec("Task IDs must be unique across splits")
    if {t.split for t in tasks} != {"learning", "selection", "held_out"}:
        raise InvalidSpec("Suite must explicitly separate learning, selection and held_out")
    return Suite(raw["id"], tuple(tasks), digest([raw, [t.identity for t in tasks]]))


@dataclass(frozen=True)
class Variant:
    id: str
    instructions: bytes
    extensions: tuple
    identity: str


@dataclass(frozen=True)
class Plan:
    id: str
    mode: str
    suite: Path
    repetitions: int
    limits: dict
    provider: dict
    variants: tuple
    identity: str


def load_plan(path, live=False):
    raw = object_fields(document(path), ["formatVersion", "id", "mode", "suite", "repetitions",
                                              "limits", "provider", "variants"])
    number(raw["formatVersion"], 1, 1)
    label(raw["id"])
    if raw["mode"] not in ("offline", "live") or (raw["mode"] == "live") != live:
        raise InvalidSpec("Real-provider plans require --live; --live requires mode live")
    number(raw["repetitions"], 1, 10)
    limits = object_fields(raw["limits"], ["maxSteps", "timeoutSeconds", "checkTimeoutSeconds"])
    number(limits["maxSteps"], 2, 128)
    number(limits["timeoutSeconds"], 1, 600)
    number(limits["checkTimeoutSeconds"], 1, 60)
    provider = object_fields(raw["provider"], ["id", "api", "model"], ["baseUrl", "apiKeyEnv"])
    label(provider["id"])
    if provider["id"] == "openai-codex" or provider["api"] not in ("responses", "chat_completions", "anthropic"):
        raise InvalidSpec("Evaluation uses explicit API-key providers, not imported OAuth stores")
    model = object_fields(provider["model"], ["id", "maxTokens"], ["contextWindow", "reasoning", "images"])
    if not isinstance(model["id"], str) or not model["id"] or len(model["id"]) > 256:
        raise InvalidSpec("Invalid model ID")
    number(model["maxTokens"], 1, 65536)
    if "contextWindow" in model:
        number(model["contextWindow"], model["maxTokens"] + 1, 10_000_000)
    for flag in ("reasoning", "images"):
        if flag in model and type(model[flag]) is not bool:
            raise InvalidSpec("Invalid model capability")
    if raw["mode"] == "offline":
        if provider["api"] != "responses" or "baseUrl" in provider or "apiKeyEnv" in provider:
            raise InvalidSpec("Offline plans use only the built-in local Responses fixture")
    else:
        from urllib.parse import urlsplit
        endpoint = provider.get("baseUrl")
        key_env = provider.get("apiKeyEnv")
        if not isinstance(endpoint, str) or not isinstance(key_env, str):
            raise InvalidSpec("Live provider needs explicit baseUrl and apiKeyEnv strings")
        url = urlsplit(endpoint)
        if (url.scheme != "https" or not url.hostname or url.username or url.password
                or url.query or url.fragment):
            raise InvalidSpec("Live provider baseUrl must be explicit HTTPS without credentials/query")
        if not re.fullmatch(r"[A-Z][A-Z0-9_]{0,48}_API_KEY", key_env):
            raise InvalidSpec("Declare a provider environment variable ending in _API_KEY")
    if not isinstance(raw["variants"], list) or not 2 <= len(raw["variants"]) <= 4:
        raise InvalidSpec("Declare baseline and 1..3 candidate or initial variants")
    variants = []
    for data in raw["variants"]:
        object_fields(data, ["id", "instructions", "extensions"])
        label(data["id"])
        instructions = read_file(input_path(path.parent, data["instructions"]), 65536)
        instructions.decode("utf-8")
        if not isinstance(data["extensions"], list) or len(data["extensions"]) > 32:
            raise InvalidSpec("Expected 0..32 frozen extension source directories")
        extensions = []
        for source in data["extensions"]:
            root = input_path(path.parent, source)
            label(root.name)
            files = inventory(root)
            if not any(f.path == "extension.json" for f in files):
                raise InvalidSpec("Extension source needs extension.json")
            if any(f.path.startswith(".versions/") for f in files):
                raise InvalidSpec("Supply extension source, not a managed store")
            extensions.append((root.name, files))
        if len({name for name, _ in extensions}) != len(extensions):
            raise InvalidSpec("Duplicate extension source ID")
        identity = digest([hashlib.sha256(instructions).hexdigest(),
                           [[name, [f.identity() for f in files]] for name, files in extensions]])
        variants.append(Variant(data["id"], instructions, tuple(extensions), identity))
    if variants[0].id != "baseline" or len({v.id for v in variants}) != len(variants):
        raise InvalidSpec("First variant must be baseline; variant IDs must be unique")
    return Plan(raw["id"], raw["mode"], input_path(path.parent, raw["suite"]).resolve(),
                raw["repetitions"], limits, provider, tuple(variants),
                digest([raw, [v.identity for v in variants]]))
