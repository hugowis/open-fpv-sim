"""Run from tools/sitegen (or with PYTHONPATH=tools/sitegen): python -m sitegen build|fetch <recipe>, walls <recipe> <way id>."""
import argparse
import sys
from pathlib import Path

from . import osm
from .build import BuildError, build_site, describe_walls
from .recipe import RecipeError


def main(argv=None) -> int:
    for stream in (sys.stdout, sys.stderr):  # a piped Windows stream is cp1252 and cannot print Cyrillic
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(prog="sitegen")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("build", help="recipe + snapshot -> .glb + world file").add_argument("recipe")
    sub.add_parser("fetch", help="download the OSM snapshot the recipe names").add_argument("recipe")
    w = sub.add_parser("walls", help="list the walls of a footprint, to write its openings")
    w.add_argument("recipe")
    w.add_argument("way_id", type=int)  # a negative id (a relation ring) parses as a number, not a flag
    args = ap.parse_args(argv)
    if args.cmd == "fetch":
        return osm.fetch_main(Path(args.recipe))
    if args.cmd == "walls":
        try:
            print("\n".join(describe_walls(Path(args.recipe), args.way_id)))
        except (RecipeError, BuildError) as e:
            print(f"walls: {args.recipe}:\n{e.args[0]}", file=sys.stderr)
            return 1
        return 0
    try:
        print(build_site(Path(args.recipe)))
    except (RecipeError, BuildError) as e:
        print(f"build: {args.recipe}:\n{e.args[0]}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
