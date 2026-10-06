extends SceneTree
## Runs the unit tests: godot --headless --path godot -s res://tests/run_tests.gd
## Exit code 0 when every check passed. (Run `godot --headless --path godot --import` once first.)

const SUITES := [
	"res://tests/test_controls.gd",
	"res://tests/test_settings.gd",
]


func _initialize() -> void:
	_run()


func _run() -> void:
	var failures := 0
	var checks := 0
	for path in SUITES:
		var suite = load(path).new()
		suite.tree = self
		var failed: int = await suite.run_all()
		print("%s: %d checks, %d failed" % [path, suite.checks, failed])
		failures += failed
		checks += suite.checks
	print("TOTAL: %d checks, %d failed" % [checks, failures])
	quit(1 if failures > 0 else 0)
