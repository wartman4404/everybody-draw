precision lowp float;
varying float time;
varying float size;
varying vec3 color;
varying vec2 position;
uniform sampler2D texture;
uniform sampler2D backbuffer;
uniform vec2 texturesize;
uniform mat4 textureMatrix;
void main() {
  vec2 uv = vec2(gl_FragCoord * textureMatrix) * vec2(0.5, -0.5);
  vec3 old = vec3(texture2D(backbuffer, uv));
  float alpha = texture2D(texture, gl_PointCoord).a;
  // I have absolutely no idea why these must be done separately, but it's the only approach that works
  float ir = 1.0 - old.r;
  float ig = 1.0 - old.g;
  float ib = 1.0 - old.b;
  vec3 newcolor = vec3(ir, ig, ib) * alpha;
  gl_FragColor = vec4(newcolor, alpha);
}
