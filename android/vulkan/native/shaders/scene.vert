#version 450
layout(location=0) in vec3 position;
layout(location=1) in vec3 normal;
layout(location=2) in vec2 texcoord;
layout(push_constant) uniform Params { vec4 model; vec4 projection; } params;
layout(location=0) out vec3 norm;
layout(location=1) out vec2 uv;
void main() {
    float a=params.model.x, c=cos(a), s=sin(a);
    mat3 r=mat3(c,0,-s, s*s,c,s*c, s*c,-s,c*c);
    vec3 p=r*position*params.model.w+vec3(params.model.yz,-5.0);
    float f=2.414213562;
    gl_Position=vec4(p.x*f/params.projection.x,p.y*f,-1.002004*p.z-0.1002004,-p.z);
    norm=r*normal; uv=texcoord;
}
