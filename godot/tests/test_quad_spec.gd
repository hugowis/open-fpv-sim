extends "res://tests/testing.gd"
## The quad spec reader: the frame part's geometry (arm, spin directions), scanned the way catalog.gd scans
## a world file — see quad_spec.gd.

const AppSettings = preload("res://scripts/settings.gd")
const QuadSpec = preload("res://scripts/quad_spec.gd")


func test_the_shipped_frames_report_their_arms_and_directions() -> void:
	var root: String = AppSettings.repo_root()
	near(QuadSpec.arm_m(root.path_join("quads/opendrone-3t-toothpick.toml")), 0.044, "the toothpick's 125 mm frame")
	near(QuadSpec.arm_m(root.path_join("quads/opendrone-5f-freestyle.toml")), 0.08, "the freestyle's 220 mm frame")
	near(QuadSpec.arm_m(root.path_join("quads/opendrone-7lr-longrange.toml")), 0.113, "the long-range's 320 mm frame")
	eq(QuadSpec.spin(root.path_join("quads/opendrone-5r-race.toml")), [1.0, -1.0, -1.0, 1.0], "Betaflight quad-X directions")


func test_an_unreadable_quad_falls_back_to_the_authored_5f() -> void:
	var dir := "user://quad-spec-test"
	DirAccess.make_dir_recursive_absolute(dir)
	var path: String = dir.path_join("bare.toml")
	FileAccess.open(path, FileAccess.WRITE).store_string("name = \"Bare\"\n")
	near(QuadSpec.arm_m(path), 0.08, "no frame part: the authored arm")
	eq(QuadSpec.spin(path), [1.0, -1.0, -1.0, 1.0], "the authored directions")
	eq(QuadSpec.arm_m("no-such-file.toml"), 0.08, "a missing quad file too")
	DirAccess.remove_absolute(path)


func test_a_frame_part_beside_its_quads_is_found() -> void:
	var dir := "user://quad-spec-lib"
	DirAccess.make_dir_recursive_absolute(dir.path_join("quads"))
	DirAccess.make_dir_recursive_absolute(dir.path_join("library/frames"))
	FileAccess.open(dir.path_join("quads/x.toml"), FileAccess.WRITE).store_string("frame = \"my-frame\"\n")
	FileAccess.open(dir.path_join("library/frames/my-frame.toml"), FileAccess.WRITE).store_string(
		"[frame]\nmotor_positions_frd_m = [[-0.05, 0.06, 0.0], [0.05, 0.06, 0.0], [-0.05, -0.06, 0.0], [0.05, -0.06, 0.0]]\nmotor_spin = [-1, 1, 1, -1]\n")
	near(QuadSpec.arm_m(dir.path_join("quads/x.toml")), 0.06, "the library beside the quads, like ofs-config")
	eq(QuadSpec.spin(dir.path_join("quads/x.toml")), [-1.0, 1.0, 1.0, -1.0], "reversed directions read too")
