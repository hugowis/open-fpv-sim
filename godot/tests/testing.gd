extends RefCounted
## A tiny assertion helper for the headless tests (`godot --headless -s res://tests/run_tests.gd`).
## A test file extends this and defines `test_*` methods (they may `await`) that call `ok`, `eq`, `near`.

var failures := 0
var checks := 0
var current := ""
## The SceneTree the suite runs in, for tests that need nodes in a tree.
var tree: SceneTree = null


func ok(condition: bool, message: String) -> void:
	checks += 1
	if not condition:
		failures += 1
		printerr("  FAIL [%s] %s" % [current, message])


func eq(actual, expected, message: String) -> void:
	ok(typeof(actual) == typeof(expected) and actual == expected, "%s: expected %s, got %s" % [message, str(expected), str(actual)])


func near(actual: float, expected: float, message: String, eps := 1e-6) -> void:
	ok(absf(actual - expected) <= eps, "%s: expected %s, got %s" % [message, str(expected), str(actual)])


## Adds a node to the tree and waits for its `_ready` (which runs on the next frame).
func add_to_tree(node: Node) -> void:
	tree.root.add_child(node)
	await tree.process_frame


## Runs every method whose name starts with `test_` and returns the number of failed checks.
func run_all() -> int:
	for method in get_method_list():
		var name: String = method["name"]
		if name.begins_with("test_"):
			current = name
			await call(name)
	return failures
