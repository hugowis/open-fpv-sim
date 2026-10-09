shader_type canvas_item;
// The analog 5.8 GHz video breaking up: grain, FM sparkles, colour loss, tearing and rolling, applied to everything
// drawn below this layer (the 3D view, the lens and Betaflight's OSD). Every amount comes from the simulator's link
// model; this shader only draws it. It never flashes the whole screen: static is fine grain around mid-grey.

uniform sampler2D screen_tex : hint_screen_texture, repeat_enable, filter_linear;
uniform float noise = 0.0;     // grain: 0 clean, 1 full static
uniform float sparkles = 0.0;  // density of FM threshold sparkles, 0 to 1
uniform float chroma = 1.0;    // colour saturation, 1 full, 0 black and white
uniform float tear = 0.0;      // 1 while the sync is unstable: line jitter and a drifting tear band
uniform float roll = 0.0;      // vertical picture offset (0 to 1) while the sync is lost
uniform float tear_at = 0.0;   // where the tear band is (0 top, 1 bottom); it drifts down while the sync is unstable
uniform float seed = 0.0;      // changes every rendered frame

// Analog lines the effects work on (a PAL field has about 288).
const float LINES = 288.0;
// Sparkle cells per line.
const float SPARKLE_CELLS = 96.0;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
}

void fragment() {
	vec2 uv = SCREEN_UV;
	float line = floor(uv.y * LINES);
	// Tearing: every line jitters sideways a little; inside a band drifting down the picture it shifts a lot.
	float band = 1.0 - smoothstep(0.0, 0.05, abs(uv.y - tear_at));
	uv.x += tear * ((hash(vec2(line, 3.0)) - 0.5) * 0.008 + band * 0.05);
	// Rolling: the picture slides up through the frame.
	uv.y = fract(uv.y + roll);
	vec3 color = texture(screen_tex, uv).rgb;
	// Colour fades to black and white, bleeding sideways on the way.
	vec3 bled = texture(screen_tex, uv + vec2(0.006 * (1.0 - chroma), 0.0)).rgb;
	color = mix(color, vec3(bled.r, color.g, bled.b), 1.0 - chroma);
	float luma = dot(color, vec3(0.299, 0.587, 0.114));
	color = mix(vec3(luma), color, chroma);
	// Grain, then static: fine noise around mid-grey takes the picture over.
	float g = hash(floor(uv * vec2(1.0 / SCREEN_PIXEL_SIZE.x, LINES * 2.0)));
	color += (g - 0.5) * 0.5 * noise;
	color = mix(color, vec3(0.3 + 0.35 * g), smoothstep(0.6, 1.0, noise));
	// Sparkles: short white or black streaks along a line.
	float cell = floor(uv.x * SPARKLE_CELLS);
	if (hash(vec2(cell, line)) > 1.0 - sparkles * 0.03) {
		color = vec3(step(0.5, hash(vec2(line, cell + 11.0))));
	}
	COLOR = vec4(clamp(color, 0.0, 1.0), 1.0);
}
