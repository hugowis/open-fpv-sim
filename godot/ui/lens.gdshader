shader_type canvas_item;
// The FPV camera's lens: barrel distortion, exposure and a vignette, applied to everything drawn below this layer.
// The picture is zoomed just enough that the distorted image still fills the screen (no black corners).

uniform sampler2D screen_tex : hint_screen_texture, repeat_disable, filter_linear;
uniform float k1 = 0.06;
uniform float k2 = 0.0;
uniform float exposure = 1.0;
uniform float vignette = 0.25;

void fragment() {
	float aspect = SCREEN_PIXEL_SIZE.y / SCREEN_PIXEL_SIZE.x;
	vec2 p = (SCREEN_UV - 0.5) * 2.0;
	p.x *= aspect;
	float corner2 = aspect * aspect + 1.0;
	float fill = 1.0 / (1.0 + k1 * corner2 + k2 * corner2 * corner2);
	float r2 = dot(p, p);
	vec2 q = p * fill * (1.0 + k1 * r2 + k2 * r2 * r2);
	q.x /= aspect;
	vec2 uv = q * 0.5 + 0.5;
	vec3 color = texture(screen_tex, uv).rgb * exposure;
	color *= 1.0 - vignette * smoothstep(0.35, 1.0, r2 / corner2);
	COLOR = vec4(color, 1.0);
}