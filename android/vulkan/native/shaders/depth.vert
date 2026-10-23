#version 450
layout(push_constant) uniform Params { vec4 color; vec4 plane; } params;
layout(location=0) out vec4 tint;
void main() {
    vec2 p=vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
    gl_Position=vec4(p*2.0-1.0,params.plane.x,1.0);
    tint=params.color;
}
