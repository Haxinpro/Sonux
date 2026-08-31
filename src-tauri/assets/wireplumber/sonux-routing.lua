-- Generated and installed by Sonux. The route data is supplied dynamically
-- through PipeWire metadata; this script only owns pre-link policy.

local cutils = require ("common-utils")
local lutils = require ("linking-utils")
local log = Log.open_topic ("sonux-routing")

local ROUTES_KEY = "sonux.routes.v1"
local ROUTES_ACK_KEY = "sonux.routes.ack.v1"
local ROUTES_READY_KEY = "sonux.routes.ready.v1"
local ROUTES_CLEAR_ACK = "cleared"
local ROUTES_VERSION = 1

local function read_routes ()
  local metadata = cutils.get_default_metadata_object ()
  if not metadata then
    return nil
  end

  local value = metadata:find (0, ROUTES_KEY)
  if not value then
    return nil
  end

  local raw = Json.Raw (value)
  if not raw or not raw:is_object () then
    log:warning ("ignoring malformed " .. ROUTES_KEY .. " metadata")
    return nil
  end

  local decoded = raw:parse ()
  if type (decoded) ~= "table" or decoded.version ~= ROUTES_VERSION or
      type (decoded.routes) ~= "table" then
    log:warning ("ignoring unsupported " .. ROUTES_KEY .. " metadata")
    return nil
  end
  return decoded.routes
end

local GENERIC_NAMES = {
  ["webrtc voiceengine"] = true, ["audio-src"] = true,
  ["playback stream"] = true, ["playstream"] = true,
  ["audio stream"] = true, ["audio player"] = true,
  ["media player"] = true, ["output"] = true, ["playback"] = true,
  ["alsa playback"] = true, ["audio output"] = true,
  ["audio source"] = true,
}

local WRAPPER_NAMES = {
  ["chromium"] = true, ["google chrome"] = true, ["chrome"] = true,
  ["electron"] = true, ["wine"] = true, ["wine64-preloader"] = true,
  ["java"] = true, ["python"] = true, ["python3"] = true,
  ["node"] = true, ["mono"] = true, ["dotnet"] = true,
  ["qtwebengine"] = true, ["cef"] = true,
}

local function identity_quality (value)
  local normalized = value:lower ()
  if GENERIC_NAMES [normalized] then return 0 end
  if WRAPPER_NAMES [normalized] then return 1 end
  return 2
end

-- Keep this property chain and quality rule aligned with Rust's
-- audio::types::resolve_identity. The persisted assignment is keyed by that
-- one authoritative identity, not by the first arbitrary matching property.
local function resolved_identity (stream_props)
  local chain = { "application.name", "application.process.binary",
      "media.name", "node.name" }
  local best_prop, best_value, best_quality = nil, nil, -1
  for _, prop in ipairs (chain) do
    local value = stream_props [prop]
    if type (value) == "string" and not value:match ("^%s*$") then
      local quality = identity_quality (value)
      if quality > best_quality then
        best_prop, best_value, best_quality = prop, value, quality
        if quality == 2 then break end
      end
    end
  end
  return best_prop, best_value
end

local function assigned_sink (routes, stream_props)
  local match_prop, match_value = resolved_identity (stream_props)
  if not match_prop then return nil end
  for _, route in ipairs (routes) do
    if type (route) == "table" and
        type (route.match_prop) == "string" and
        type (route.match_value) == "string" and
        type (route.sink_name) == "string" and
        route.sink_name:match ("^sink_[%w_.%-]+$") and
        route.match_prop == match_prop and route.match_value == match_value then
      return route.sink_name
    end
  end
  return nil
end

-- Acknowledge only after this WirePlumber process has received the metadata
-- change. Sonux waits for this before treating an assignment transaction as
-- committed, so a newly created stream cannot overtake the route update.
SimpleEventHook {
  name = "sonux/acknowledge-routes",
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "metadata-changed" },
      Constraint { "metadata.name", "=", "default" },
      Constraint { "event.subject.key", "=", ROUTES_KEY },
    },
  },
  execute = function (event)
    local props = event:get_properties ()
    local value = props ["event.subject.value"]
    if value then
      local raw = Json.Raw (value)
      if not raw or not raw:is_object () then
        return
      end
      local decoded = raw:parse ()
      if type (decoded) ~= "table" or decoded.version ~= ROUTES_VERSION or
          type (decoded.routes) ~= "table" then
        return
      end
    end

    local metadata = cutils.get_default_metadata_object ()
    if metadata then
      metadata:set (0, ROUTES_ACK_KEY, "Spa:String", value or ROUTES_CLEAR_ACK)
    end
  end,
}:register ()

SimpleEventHook {
  name = "sonux/find-assigned-target",
  -- Sonux assignments are stronger than role, filter, default and best-node
  -- policy. Order before every stock selector so another hook cannot win the
  -- first-target race merely because component registration order changed.
  before = {
    "linking/find-media-role-target",
    "linking/find-defined-target",
    "linking/find-audio-group-target",
    "linking/find-filter-target",
    "linking/find-default-target",
    "linking/find-best-target",
    "linking/prepare-link",
  },
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "select-target" },
    },
  },
  execute = function (event)
    local _, om, si, stream_props, flags, target =
        lutils:unwrap_select_target_event (event)

    -- Do not fight a runtime move or an already established route. This hook
    -- only chooses the first target (and a fresh target after disconnection).
    if target or flags.peer_id or
        stream_props ["media.class"] ~= "Stream/Output/Audio" or
        stream_props ["item.node.direction"] ~= "output" then
      return
    end

    local routes = read_routes ()
    local sink_name = routes and assigned_sink (routes, stream_props)
    if not sink_name then
      return
    end

    for candidate in om:iterate { type = "SiLinkable" } do
      local target_props = candidate.properties
      if target_props ["node.name"] == sink_name and
          target_props ["media.class"] == "Audio/Sink" and
          target_props ["item.node.direction"] == "input" and
          target_props ["factory.name"] == "support.null-audio-sink" and
          target_props ["sonux.owner"] == "sonux" and
          lutils.canLink (stream_props, candidate) then
        local compatible, can_passthrough =
            lutils.checkPassthroughCompatibility (si, candidate)
        if compatible then
          flags.can_passthrough = can_passthrough
          flags.has_defined_target = true
          event:set_data ("target", candidate)
          log:info (si, "selected assigned Sonux target " .. sink_name)
        end
        return
      end
    end

    -- Never invent a fallback target here. If Sonux has not published its
    -- channel node yet, the stock policy remains responsible for the stream.
    log:warning (si, "assigned Sonux target is unavailable: " .. sink_name)
  end,
}:register ()

-- Presence is separate from the route acknowledgement. Publish it only after
-- the selector is registered, so Sonux cannot observe a ready component before
-- pre-link routing is actually active. A newly upgraded Sonux keeps using its
-- live-router fallback until the next login or session-manager restart.
local ready_metadata = cutils.get_default_metadata_object ()
if ready_metadata then
  ready_metadata:set (0, ROUTES_READY_KEY, "Spa:String", "1")
  local existing = ready_metadata:find (0, ROUTES_KEY)
  if existing and read_routes () then
    ready_metadata:set (0, ROUTES_ACK_KEY, "Spa:String", existing)
  elseif not existing then
    ready_metadata:set (0, ROUTES_ACK_KEY, "Spa:String", ROUTES_CLEAR_ACK)
  end
end
