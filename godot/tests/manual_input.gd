extends SceneTree
## Manual check on a physical Mac: prints the joypads Godot sees, with their axes and buttons.
## Run windowed (not --headless), plug or unplug a pad, and run it again:
##   godot --path godot -s res://tests/manual_input.gd
## The gamepad step in docs/research/macos-manual.md records this output.

func _initialize() -> void:
	var pads := Input.get_connected_joypads()
	print("joypads: ", pads)
	for id in pads:
		var axes := []
		for axis in range(8):
			axes.append(Input.get_joy_axis(id, axis))
		var pressed := []
		for button in range(16):
			if Input.is_joy_button_pressed(id, button):
				pressed.append(button)
		print("  %d: %s info=%s" % [id, Input.get_joy_name(id), Input.get_joy_info(id)])
		print("    axes 0-7: %s buttons pressed: %s" % [axes, pressed])
	quit()
