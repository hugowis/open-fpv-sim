shader_type spatial;
render_mode cull_back, diffuse_lambert, specular_disabled;
// The ground: flat grey-green with a 1 m grid and a stronger line every 10 m, drawn from world position so
// it needs no UVs. Lines fade out with distance (when a cell shrinks below a couple of pixels) to avoid moire.

uniform vec3 base_color : source_color = vec3(0.30, 0.34, 0.29);
uniform vec3 line_color : source_color = vec3(0.50, 0.55, 0.48);
uniform vec3 major_color : source_color = vec3(0.82, 0.84, 0.74);
uniform float cell = 1.0;
uniform float major_every = 10.0;

varying vec3 world_pos;

void vertex() {
	world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

// 1 on a line, 0 between lines; the line is about `width` pixels wide and gone when cells get tiny.
float grid(vec2 p, float size, float width) {
	vec2 q = p / size;
	vec2 w = max(fwidth(q), vec2(1e-5));
	vec2 g = abs(fract(q - 0.5) - 0.5) / w;
	float line = 1.0 - clamp(min(g.x, g.y) - (width - 1.0), 0.0, 1.0);
	float fade = 1.0 - smoothstep(0.25, 0.5, max(w.x, w.y));
	return line * fade;
}

void fragment() {
	vec2 p = world_pos.xz;
	vec3 color = base_color;
	color = mix(color, line_color, grid(p, cell, 1.0) * 0.55);
	color = mix(color, major_color, grid(p, cell * major_every, 2.0) * 0.8);
	ALBEDO = color;
	ROUGHNESS = 1.0;
}