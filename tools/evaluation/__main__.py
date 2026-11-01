import argparse
import sys

from .runner import evaluate, write_report
from .spec import InvalidSpec


def main():
    parser = argparse.ArgumentParser(description="Compare Hibiscus coding outcomes in fresh local workspaces")
    parser.add_argument("--plan", required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--split", choices=("learning", "selection", "held_out"), default="selection")
    parser.add_argument("--live", action="store_true", help="Explicitly permit the configured real-provider run")
    args = parser.parse_args()
    try:
        from pathlib import Path
        if Path(args.output).exists() or Path(args.output).is_symlink():
            raise InvalidSpec("Output already exists; choose a new report path")
        report = evaluate(args.plan, args.binary, args.split, args.live)
        write_report(report, args.output)
    except (InvalidSpec, OSError, UnicodeError, ValueError) as error:
        # Do not echo provider replies, credentials, prompts or arbitrary file content.
        print(f"evaluation: {type(error).__name__}: configuration or input/output failed", file=sys.stderr)
        return 2
    print(f"{report['mode']} {report['split']}: {len(report['results'])} runs")
    for summary in report["summary"]:
        print(f"{summary['variantId']}: {summary['solved']}/{summary['runs']} solved")
    print(f"Report: {args.output}")
    return 0 if report["structure"]["comparable"] else 2


if __name__ == "__main__":
    sys.exit(main())
