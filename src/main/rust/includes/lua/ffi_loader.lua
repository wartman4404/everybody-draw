ffi = require("ffi")
ffi.cdef[[
  struct ShaderPaintPoint {
    float x;
    float y;
    float time;
    float size;
    float speedx;
    float speedy;
    float distance;
    float counter;
  };

  void lua_pushpoint(void *output, int queue, struct ShaderPaintPoint *point);
  short lua_nextpoint(void *output, struct ShaderPaintPoint *points);
  void lua_log(const char *message);
  void lua_pushline(void *output, int queue, struct ShaderPaintPoint *pointa, struct ShaderPaintPoint *pointb);
  void lua_pushcatmullrom(void *output, int queue, struct ShaderPaintPoint points[4]);
  void lua_pushcubicbezier(void *output, int queue, struct ShaderPaintPoint points[4]);
  void lua_clearlayer(void *output, int layer);
  void lua_savelayers(void *output);
]]

pushpoint=ffi.C.lua_pushpoint
pushline=ffi.C.lua_pushline
pushcatmullrom=ffi.C.lua_pushcatmullrom
pushcubicbezier=ffi.C.lua_pushcubicbezier
loglua=ffi.C.lua_log
clearlayer=ffi.C.lua_clearlayer
savelayers=ffi.C.lua_savelayers

ShaderPaintPoint=ffi.typeof("struct ShaderPaintPoint")

local function copytable(t)
  out = {}
  for k,v in pairs(t) do
    out[k] = v
  end
  return out
end

local stringbox = copytable(string)
local mathbox = copytable(math)
local tablebox = copytable(table)
sandboxed = {
  assert = assert,
  error = error,
  ipairs = ipairs,
  next = next,
  pairs = pairs,
  pcall = pcall,
  print = loglua,
  select = select,
  tonumber = tonumber,
  tostring = tostring,
  type = type,
  unpack = unpack,
  string = stringbox,
  math = mathbox,
  table = tablebox,
  pushpoint = pushpoint,

  pushline = pushline,
  pushcatmullrom = pushcatmullrom,
  pushcubicbezier = pushcubicbezier,
  loglua = loglua,
  clearlayer = clearlayer,
  savelayers = savelayers,
  ShaderPaintPoint = ShaderPaintPoint,
}