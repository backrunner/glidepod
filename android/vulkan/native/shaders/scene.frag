#version 450
layout(set=0,binding=0) uniform sampler2D image;
layout(location=0) in vec3 norm;
layout(location=1) in vec2 uv;
layout(location=0) out vec4 color;
void main() {
    vec3 n=normalize(norm);
    float light=max(dot(n,normalize(vec3(0.5,0.7,1.0))),0.0);
    float spec=pow(max(dot(n,normalize(vec3(0.1,0.3,1.0))),0.0),24.0);
    color=vec4(texture(image,uv).rgb*(0.18+0.82*light)+vec3(0.4)*spec,1);
}
