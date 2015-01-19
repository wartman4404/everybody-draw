function main(a, b, x, y, points)
  local steps = math.max(math.abs(a.x - b.x), math.abs(a.y - b.y))
  if steps < 1 then steps = 1 end
  local timescale = 10
  local stepx = (b.x - a.x) / steps
  local stepy = (b.y - a.y) / steps
  local steptime = (b.time - a.time) / (steps * timescale)
  local stepsize = (b.size - a.size) / steps
  local stepspeed = (b.speed - a.speed) / steps
  local stepdistance = (b.distance - a.distance) / steps
  local out = ShaderPaintPoint(a.x,a.y,a.time,a.size,a.speed,a.distance,a.counter)
  for i = 1,steps do
    pushpoint(points, 1, out)
    out.x = out.x + stepx
    out.y = out.y + stepy
    out.size = out.size + stepsize
    out.speed = out.speed + stepspeed
    out.distance = out.distance + stepdistance
    out.time = out.time + steptime
    if out.time > 1 then out.time = out.time - 1 end
  end
end

