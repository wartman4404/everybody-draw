attribute vec4 vPosition;
attribute vec4 vTexCoord;
precision lowp float;
uniform mat4 textureMatrix;
varying vec2 uv;
void main() {
    uv = (textureMatrix * vTexCoord).xy;
    gl_Position = vPosition;
}
