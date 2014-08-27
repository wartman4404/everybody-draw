struct Coordinate {
  float x;
  float y;
};

struct PaintPoint {
  struct Coordinate pos;
  float time;
  float size;
};

struct ShaderPaintPoint {
  struct Coordinate pos;
  float time;
  float size;
  float speed;
  float distance;
  float counter;
};