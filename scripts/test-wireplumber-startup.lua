-- Run from the project root: lua scripts/test-wireplumber-startup.lua
-- Simulate WirePlumber loading policy hooks before its event source exists.
local hooks, values = {}, {}
local source_ready = false
local metadata = {}
function metadata:find (_, key) return values[key] end
function metadata:set (_, key, _, value) values[key] = value end
package.preload["common-utils"] = function ()
  return { get_default_metadata_object = function ()
    assert(source_ready, "metadata accessed before event source activation")
    return metadata
  end }
end
package.preload["linking-utils"] = function () return {} end
Log = { open_topic = function () return { warning = function () end } end }
Constraint = function (value) return value end
EventInterest = function (value) return value end
SimpleEventHook = function (hook)
  function hook:register () hooks[self.name] = self end
  return hook
end
Json = { Raw = function (value)
  return {
    is_object = function () return value == "valid" end,
    parse = function () return { version = 1, routes = {} } end,
  }
end }
dofile("src-tauri/assets/wireplumber/sonux-routing.lua")
assert(hooks["sonux/find-assigned-target"])
assert(next(values) == nil, "readiness published before metadata discovery")
source_ready = true
local ready = assert(hooks["sonux/metadata-ready"])
assert(ready.interests[1][1][3] == "metadata-added")
assert(ready.interests[1][2][3] == "default")
ready.execute({})
assert(values["sonux.routes.ready.v1"] == "1")
assert(values["sonux.routes.ack.v1"] == "cleared")
values = { ["sonux.routes.v1"] = "valid" }
ready.execute({})
assert(values["sonux.routes.ack.v1"] == "valid")
values = { ["sonux.routes.v1"] = "malformed" }
ready.execute({})
assert(values["sonux.routes.ack.v1"] == nil)
print("WirePlumber startup, readiness, and retained-route acknowledgement passed")
