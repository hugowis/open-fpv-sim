"""Run from tools/sitegen (or with PYTHONPATH=tools/sitegen): python -m sitegen build|fetch <recipe>."""
import argparse
import sys
from pathlib import Path

from . import osm
from .build import BuildError, build_site
from .recipe import RecipeError


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="sitegen")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("build", help="recipe + snapshot -> .glb + world file").add_argument("recipe")
    sub.add_parser("fetch", help="download the OSM snapshot the recipe names").add_argument("recipe")
    args = ap.parse_args(argv)
    if args.cmd == "fetch":
        return osm.fetch_main(Path(args.recipe))
    try:
        print(build_site(Path(args.recipe)))
    except (RecipeError, BuildError) as e:
        print(f"build: {args.recipe}:\n{e.args[0]}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
