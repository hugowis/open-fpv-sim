extends CanvasLayer
## The FPV lens: a full-screen shader over the 3D view (barrel distortion, exposure, vignette). The HUD sits on a
## layer above this one, so it is not distorted. Hidden in the chase view.

const LENS_SHADER := preload("res://ui/lens.gdshader")

@export_range(-0.5, 0.8) var distortion := 0.06:
	set(value):
		distortion = value
		_apply()
@export_range(0.2, 3.0) var exposure := 1.0:
	set(value):
		exposure = value
		_apply()
@export_range(0.0, 1.0) var vignette := 0.25:
	set(value):
		vignette = value
		_apply()

var _material := ShaderMaterial.new()


func _ready() -> void:
	_material.shader = LENS_SHADER
	var rect := ColorRect.new()
	rect.name = "LensRect"
	rect.set_anchors_preset(Control.PRESET_FULL_RECT)
	rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	rect.material = _material
	add_child(rect)
	_apply()


func set_enabled(enabled: bool) -> void:
	visible = enabled


func _apply() -> void:
	_material.set_shader_parameter("k1", distortion)
	_material.set_shader_parameter("exposure", exposure)
	_material.set_shader_parameter("vignette", vignette)
