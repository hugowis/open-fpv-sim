extends "res://tests/testing.gd"
## The quad preview: a turning model of the selected quad in a small viewport (the main menu's now, the
## workbench's later, #128).

const AppSettings = preload("res://scripts/settings.gd")
const QuadPreview = preload("res://ui/quad_preview.gd")


func test_the_preview_shows_a_turning_model_of_the_quad() -> void:
	var preview := QuadPreview.new()
	await add_to_tree(preview)
	await tree.process_frame  # the model's _ready
	ok(preview.model() != null and preview.model().get_script() == preload("res://drone/drone.gd"), "a drone model")
	near(preview.model()._model.scale.x, 1.0, "the authored 5F at first")
	ok(preview.mouse_filter == Control.MOUSE_FILTER_IGNORE, "the preview never takes the mouse or the focus")
	var root: String = AppSettings.repo_root()
	preview.set_quad(root.path_join("quads/opendrone-3t-toothpick.toml"))
	await tree.process_frame
	ok(preview.model() != null, "the model was rebuilt")
	near(preview.model()._model.scale.x, 0.044 / 0.08, "at the toothpick's size", 1e-4)
	var before: float = preview.model().global_rotation.y
	preview._process(0.1)
	ok(preview.model().global_rotation.y > before, "the turntable turns the model")
	preview.queue_free()
