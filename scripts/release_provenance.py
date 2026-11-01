#!/usr/bin/env python3
"""Release attestation policy, used by trusted CI and explicit user verification.

Default installation remains checksum-only. No downloaded archive is executed.
"""
import argparse
from dataclasses import dataclass
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from typing import Union

REPOSITORY = "Rekabytes-Enterprise/hibiscus"
WORKFLOW = f"{REPOSITORY}/.github/workflows/release.yml"
PREDICATE_TYPE = "https://slsa.dev/provenance/v1"
BUILD_TYPE = (f"https://github.com/{REPOSITORY}/blob/main/"
              "docs/development/release-provenance.md#build-type-v1")
TARGETS = ("aarch64-apple-darwin", "aarch64-unknown-linux-musl",
           "x86_64-apple-darwin", "x86_64-unknown-linux-musl")
ARCHIVES = tuple(f"hibiscus-{target}.tar.gz" for target in TARGETS)
VERIFIER_VERSION = "2.103.3"
# Official GitHub CLI release archive digests, reviewed with the tool version.
VERIFIER_ARCHIVES = {
    ("Linux", "x86_64"): ("linux_amd64", "tar.gz", "4c806978ffce1cec70853302a56d70ac6da4f19bae7adca4e1a8ab00633dd91d"),
    ("Linux", "aarch64"): ("linux_arm64", "tar.gz", "ee32df2323c8847f5fac6cb6b4b69ca75828e9329a8f22f677021f7b3306ce63"),
    ("Darwin", "x86_64"): ("macOS_amd64", "zip", "a2f86418f4350ae6e9ec60d5000bdf7665728436946a97fc27fe50e6ae752682"),
    ("Darwin", "arm64"): ("macOS_arm64", "zip", "d700207eae9ae98fe2b89535c2c087c07823d4628c56364695a5a07ef311c062"),
}


class InvalidProvenance(ValueError):
    """A release input or verified statement violates the trust contract."""


def require(condition, message):
    if not condition:
        raise InvalidProvenance(message)


def hex_digest(value, length):
    require(isinstance(value, str) and re.fullmatch(f"[0-9a-f]{{{length}}}", value),
            "Invalid digest")
    return value


def tag(value):
    require(isinstance(value, str) and re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value),
            "Expected an immutable vMAJOR.MINOR.PATCH tag")
    return value


@dataclass(frozen=True)
class ExpectedRelease:
    tag: str
    source_commit: str
    workflow_ref: str
    workflow_commit: str


@dataclass(frozen=True)
class BuildContext:
    release: ExpectedRelease
    source_sha256: str
    event: str
    run_id: str
    attempt: str


@dataclass(frozen=True)
class Artifact:
    name: str
    sha256: str


def expected_release(release_tag, source_commit, workflow_ref=None, workflow_commit=None):
    release_tag = tag(release_tag)
    source_commit = hex_digest(source_commit, 40)
    workflow_ref = workflow_ref or f"refs/tags/{release_tag}"
    if workflow_ref == f"refs/tags/{release_tag}":
        workflow_commit = workflow_commit or source_commit
        require(workflow_commit == source_commit, "Tag workflow must build its own commit")
    else:
        require(workflow_ref in ("refs/heads/dev", "refs/heads/main"), "Untrusted workflow ref")
        require(workflow_commit is not None, "Recovery verification needs the reviewed workflow commit")
    return ExpectedRelease(release_tag, source_commit, workflow_ref,
                           hex_digest(workflow_commit, 40))


def build_context(release, source_sha256, event, run_id, attempt):
    require(event in ("push", "workflow_dispatch"), "Unsupported release event")
    if event == "push":
        require(release.workflow_ref == f"refs/tags/{release.tag}", "Tag push identity mismatch")
    else:
        require(release.workflow_ref in ("refs/heads/dev", "refs/heads/main"),
                "Recovery must use a protected workflow branch")
    for value in (run_id, attempt):
        require(isinstance(value, str) and re.fullmatch(r"[1-9][0-9]{0,19}", value),
                "Invalid workflow run identity")
    return BuildContext(release, hex_digest(source_sha256, 64), event, run_id, attempt)


def context_from_env():
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY, "Wrong repository")
    release = expected_release(os.environ["RELEASE_TAG"], os.environ["SOURCE_COMMIT"],
                               os.environ["GITHUB_REF"], os.environ["GITHUB_WORKFLOW_SHA"])
    require(os.environ["GITHUB_WORKFLOW_REF"] == f"{WORKFLOW}@{release.workflow_ref}",
            "Wrong signer workflow")
    require(os.environ["GITHUB_SHA"] == release.workflow_commit, "Workflow invocation mismatch")
    return build_context(release, os.environ["SOURCE_SHA256"], os.environ["GITHUB_EVENT_NAME"],
                         os.environ["GITHUB_RUN_ID"], os.environ["GITHUB_RUN_ATTEMPT"])


def predicate(context: BuildContext) -> dict:
    release = context.release
    source_uri = f"git+https://github.com/{REPOSITORY}"
    return {
        "buildDefinition": {
            "buildType": BUILD_TYPE,
            "externalParameters": {"releaseTag": release.tag, "targets": list(TARGETS)},
            "internalParameters": {"github": {"event": context.event, "runId": context.run_id,
                                               "runAttempt": context.attempt}},
            "resolvedDependencies": [
                {"name": "source", "uri": f"{source_uri}@refs/tags/{release.tag}",
                 "digest": {"gitCommit": release.source_commit}},
                {"name": "workflow", "uri": f"{source_uri}@{release.workflow_ref}",
                 "digest": {"gitCommit": release.workflow_commit}}]},
        "runDetails": {
            "builder": {"id": f"https://github.com/{WORKFLOW}@{release.workflow_ref}"},
            "metadata": {"invocationId": f"https://github.com/{REPOSITORY}/actions/runs/{context.run_id}/attempts/{context.attempt}"},
            "byproducts": [{"name": "verified-source/source.tar",
                            "digest": {"sha256": context.source_sha256}}]}}


def context_from_predicate(value):
    try:
        definition = value["buildDefinition"]
        dependencies = definition["resolvedDependencies"]
        require(isinstance(dependencies, list) and len(dependencies) == 2, "Invalid source dependencies")
        release_tag = definition["externalParameters"]["releaseTag"]
        prefix = f"git+https://github.com/{REPOSITORY}@"
        workflow_uri = dependencies[1]["uri"]
        require(isinstance(workflow_uri, str) and workflow_uri.startswith(prefix), "Wrong workflow repository")
        release = expected_release(release_tag, dependencies[0]["digest"]["gitCommit"],
                                   workflow_uri[len(prefix):], dependencies[1]["digest"]["gitCommit"])
        github = definition["internalParameters"]["github"]
        context = build_context(release, value["runDetails"]["byproducts"][0]["digest"]["sha256"],
                                github["event"], github["runId"], github["runAttempt"])
        require(predicate(context) == value, "Unexpected provenance claims")
        return context
    except (KeyError, IndexError, TypeError) as error:
        raise InvalidProvenance("Malformed provenance claims") from error


def regular_file(path, maximum):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and 0 < info.st_size <= maximum, "Invalid input file")


def artifact(path):
    require(path.name in ARCHIVES, "Unsupported archive name")
    regular_file(path, 256 * 1024 * 1024)
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return Artifact(path.name, digest.hexdigest())


def collect(directory=None, archive=None):
    if directory is not None:
        require(not directory.is_symlink() and directory.is_dir(), "Invalid archive directory")
        paths = sorted(directory.iterdir())
        require({p.name for p in paths} == set(ARCHIVES) and len(paths) == len(ARCHIVES),
                "Expected exactly four release archives")
    else:
        paths = [archive]
    return {p: artifact(p) for p in paths}


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, "Duplicate JSON field")
        value[key] = item
    return value


def verified_statement(output, expected, artifacts):
    require(len(output) <= 8 * 1024 * 1024, "Verifier output too large")
    try:
        results = json.loads(output, object_pairs_hook=unique_object)
        require(isinstance(results, list) and len(results) == 1, "Expected exactly one verified attestation")
        statement = results[0]["verificationResult"]["statement"]
        require(set(statement) == {"_type", "predicateType", "subject", "predicate"},
                "Unexpected statement fields")
        require(statement["_type"] == "https://in-toto.io/Statement/v1" and
                statement["predicateType"] == PREDICATE_TYPE, "Wrong statement type")
        context = context_from_predicate(statement["predicate"])
        release = expected.release if isinstance(expected, BuildContext) else expected
        require(context.release == release,
                "Source/tag/workflow identity mismatch")
        if isinstance(expected, BuildContext):
            require(context == expected, "Build run/source-snapshot identity mismatch")
        subjects = statement["subject"]
        require(isinstance(subjects, list) and len(subjects) == len(ARCHIVES), "Wrong subject count")
        actual = {}
        for subject in subjects:
            require(set(subject) == {"name", "digest"} and set(subject["digest"]) == {"sha256"},
                    "Unexpected subject fields")
            name = subject["name"]
            require(name in ARCHIVES and name not in actual, "Wrong or duplicate archive subject")
            actual[name] = hex_digest(subject["digest"]["sha256"], 64)
        require(set(actual) == set(ARCHIVES), "Missing release subjects")
        require(all(actual[a.name] == a.sha256 for a in artifacts.values()), "Archive digest mismatch")
    except (json.JSONDecodeError, KeyError, TypeError) as error:
        raise InvalidProvenance("Malformed verified statement") from error


def verifier_environment(temporary):
    # Neither account/provider credentials nor the attestor's OIDC grant reaches gh.
    env = {key: os.environ[key] for key in
           ("PATH", "TMPDIR", "HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "NO_PROXY",
            "SSL_CERT_FILE", "SSL_CERT_DIR") if key in os.environ}
    env.update(HOME=temporary, GH_CONFIG_DIR=temporary, GH_PROMPT_DISABLED="1", NO_COLOR="1")
    return env


def verify_release(artifacts: dict, bundle: Path, expected: Union[ExpectedRelease, BuildContext],
                   verifier: Path) -> None:
    regular_file(bundle, 2 * 1024 * 1024)
    require(bool(artifacts), "No archives selected")
    release = expected.release if isinstance(expected, BuildContext) else expected
    command = [str(verifier), "attestation", "verify", str(next(iter(artifacts))),
               "--hostname", "github.com", "--bundle", str(bundle), "--repo", REPOSITORY,
               "--cert-oidc-issuer", "https://token.actions.githubusercontent.com",
               "--cert-identity", f"https://github.com/{WORKFLOW}@{release.workflow_ref}",
               "--signer-digest", release.workflow_commit,
               "--source-ref", release.workflow_ref, "--source-digest", release.workflow_commit,
               "--deny-self-hosted-runners", "--predicate-type", PREDICATE_TYPE, "--format", "json"]
    # A local bundle does not require account credentials or a login profile.
    with tempfile.TemporaryDirectory(prefix="hibiscus-attestation-") as temporary:
        result = subprocess.run(command, env=verifier_environment(temporary), stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=120)
    require(result.returncode == 0, "Cryptographic provenance verification failed")
    # Only parsed output from a successful cryptographic verifier reaches policy.
    verified_statement(result.stdout, expected, artifacts)


def install_verifier(directory):
    selection = VERIFIER_ARCHIVES.get((platform.system(), platform.machine()))
    require(selection is not None, "Unsupported CI verifier platform")
    target, extension, expected = selection
    root = f"gh_{VERIFIER_VERSION}_{target}"
    url = f"https://github.com/cli/cli/releases/download/v{VERIFIER_VERSION}/{root}.{extension}"
    request = urllib.request.Request(url, headers={"User-Agent": "hibiscus-provenance-ci"})
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read(256 * 1024 * 1024 + 1)
    require(len(data) <= 256 * 1024 * 1024 and hashlib.sha256(data).hexdigest() == expected,
            "CI verifier archive digest mismatch")
    member = f"{root}/bin/gh"
    if extension == "zip":
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            info = archive.getinfo(member)
            require(info.file_size <= 256 * 1024 * 1024, "CI verifier too large")
            binary = archive.read(member)
    else:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            info = archive.getmember(member)
            require(info.isfile() and info.size <= 256 * 1024 * 1024, "Invalid CI verifier member")
            binary = archive.extractfile(info).read()
    directory.mkdir(mode=0o700)
    destination = directory / "gh"
    with destination.open("xb") as output:
        output.write(binary)
    destination.chmod(0o755)
    with tempfile.TemporaryDirectory(prefix="hibiscus-verifier-version-") as temporary:
        result = subprocess.run([str(destination), "--version"], env=verifier_environment(temporary),
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
    require(result.returncode == 0 and result.stdout.startswith(f"gh version {VERIFIER_VERSION} ".encode()),
            "Wrong CI verifier version")
    return destination.resolve()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare", help="Create canonical predicate from trusted workflow inputs")
    prepare.add_argument("--directory", type=Path, required=True)
    prepare.add_argument("--output", type=Path, required=True)
    tool = commands.add_parser("verifier", help="Install a checksum-pinned CI verifier into a new directory")
    tool.add_argument("--directory", type=Path, required=True)
    verify = commands.add_parser("verify", help="Verify local archives and signed release identity; never installs them")
    selection = verify.add_mutually_exclusive_group(required=True)
    selection.add_argument("--directory", type=Path)
    selection.add_argument("--archive", type=Path)
    verify.add_argument("--bundle", type=Path, required=True)
    verify.add_argument("--context-env", action="store_true")
    verify.add_argument("--tag")
    verify.add_argument("--source-commit")
    verify.add_argument("--workflow-ref")
    verify.add_argument("--workflow-commit")
    verify.add_argument("--verifier", type=Path, default=Path("gh"))
    args = parser.parse_args()
    try:
        if args.command == "verifier":
            print(install_verifier(args.directory))
        elif args.command == "prepare":
            collect(directory=args.directory)
            value = predicate(context_from_env())
            args.output.parent.mkdir(parents=True, exist_ok=True)
            with args.output.open("x") as stream:
                json.dump(value, stream, sort_keys=True)
                stream.write("\n")
        else:
            if args.context_env:
                require(not any([args.tag, args.source_commit, args.workflow_ref, args.workflow_commit]),
                        "Choose explicit identity or workflow environment")
                expected = context_from_env()
            else:
                expected = expected_release(args.tag, args.source_commit, args.workflow_ref, args.workflow_commit)
            files = collect(args.directory, args.archive)
            verify_release(files, args.bundle, expected, args.verifier)
            release = expected.release if isinstance(expected, BuildContext) else expected
            print(f"Verified provenance for {len(files)} archive(s) from {release.tag} at {release.source_commit}.")
    except (InvalidProvenance, OSError, KeyError, subprocess.SubprocessError, ValueError,
            tarfile.TarError, zipfile.BadZipFile) as error:
        # Do not echo subprocess output, token values or untrusted signed text.
        category = str(error) if isinstance(error, InvalidProvenance) else "Invalid input or verification dependency failure"
        print(f"hibiscus provenance: {category}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
