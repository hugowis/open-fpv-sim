extends CanvasLayer
## The analog video link's picture: a full-screen shader over the 3D view, the lens and Betaflight's OSD (layer 3,
## between the OSD on layer 2 and the HUD on layer 4), so the OSD breaks up with the picture as it does in real
## goggles. The simulator's link model decides how much grain, sparkles, colour loss, tearing and rolling there is;
## this layer only draws it. Off in the chase view, without a VTX, with the "Video effects" setting off, and while the
## link is clean.

const VIDEO_SHADER := preload("res://ui/video.gdshader")
## How fast a picture without sync rolls, in screen heights per second.
const ROLL_SPEED := 0.7
## How fast the tear band of an unstable sync drifts down the picture, in screen heights per second.
const TEAR_SPEED := 0.3

## The "Video effects" setting.
var effects := true:
	set(value):
		effects = value
		_refresh()

var _enabled := true
var _material := ShaderMaterial.new()
var _rect := ColorRect.new()
var _uniforms := uniforms_for({})
var _roll := 0.0
var _tear := 0.0


func _ready() -> void:
	_material.shader = VIDEO_SHADER
	_rect.name = "VideoRect"
	_rect.set_anchors_preset(Control.PRESET_FULL_RECT)
	_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_rect.material = _material
	add_child(_rect)
	_refresh()


## The shader's amounts for a telemetry Dictionary (the one `OfsClient.get_telemetry()` returns): `active` is false
## without a video link; `sync` is 0 locked, 1 unstable, 2 lost.
static func uniforms_for(t: Dictionary) -> Dictionary:
	if not bool(t.get("video_present", false)):
		return {"active": false, "noise": 0.0, "sparkles": 0.0, "chroma": 1.0, "sync": 0}
	var sync: int = {"unstable": 1, "lost": 2}.get(String(t.get("video_sync", "")), 0)
	return {
		"active": true,
		"noise": clampf(float(t.get("video_noise", 0.0)), 0.0, 1.0),
		"sparkles": clampf(float(t.get("video_sparkles", 0.0)), 0.0, 1.0),
		"chroma": clampf(float(t.get("video_chroma", 1.0)), 0.0, 1.0),
		"sync": sync,
	}


## True when the amounts change nothing on screen (a locked, clean link).
static func is_clean(u: Dictionary) -> bool:
	return u["noise"] <= 0.0 and u["sparkles"] <= 0.0 and u["chroma"] >= 1.0 and u["sync"] == 0


## Feeds the layer one frame's telemetry.
func update_view(t: Dictionary, delta: float) -> void:
	_uniforms = uniforms_for(t)
	_roll = fposmod(_roll + ROLL_SPEED * delta, 1.0) if _uniforms["sync"] == 2 else 0.0
	if _uniforms["sync"] == 1:
		_tear = fposmod(_tear + TEAR_SPEED * delta, 1.0)
	_material.set_shader_parameter("noise", _uniforms["noise"])
	_material.set_shader_parameter("sparkles", _uniforms["sparkles"])
	_material.set_shader_parameter("chroma", _uniforms["chroma"])
	_material.set_shader_parameter("tear", 1.0 if _uniforms["sync"] == 1 else 0.0)
	_material.set_shader_parameter("roll", _roll)
	_material.set_shader_parameter("tear_at", _tear)
	_material.set_shader_parameter("seed", randf())
	_refresh()


## Off in the chase view.
func set_enabled(enabled: bool) -> void:
	_enabled = enabled
	_refresh()


## True when the layer draws over the picture now.
func is_active() -> bool:
	return visible


## The amounts of the last `update_view` (for the tests).
func uniforms() -> Dictionary:
	return _uniforms


func _refresh() -> void:
	visible = _enabled and effects and _uniforms["active"] and not is_clean(_uniforms)
