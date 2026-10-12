-- Headless REAPER verification for ToneTinkerer (run by scripts/reaper-verify.sh).
-- Environment:
--   SKY_DI       input wav           SKY_OUT_DIR  render directory
--   SKY_FX       FX name to insert   SKY_LOG      result log path
--   SKY_SRATE    project sample rate
local di = os.getenv("SKY_DI")
local out_dir = os.getenv("SKY_OUT_DIR")
local fx_name = os.getenv("SKY_FX") or "VST3:ToneTinkerer"
local srate = tonumber(os.getenv("SKY_SRATE") or "48000")
local log = io.open(os.getenv("SKY_LOG"), "w")
local function say(s) log:write(s .. "\n"); log:flush() end

reaper.GetSetProjectInfo(0, "PROJECT_SRATE", srate, true)
reaper.GetSetProjectInfo(0, "PROJECT_SRATE_USE", 1, true)
reaper.InsertTrackAtIndex(0, true)
local tr = reaper.GetTrack(0, 0)
reaper.SetOnlyTrackSelected(tr)
reaper.SetEditCurPos(0, false, false)
say("insert_media=" .. tostring(reaper.InsertMedia(di, 0)))
local fx = reaper.TrackFX_AddByName(tr, fx_name, false, -1)
say("fx_index=" .. tostring(fx))
if fx < 0 then say("render_done=false"); log:close(); return end
local _, name = reaper.TrackFX_GetFXName(tr, fx, "")
say("fx_name=" .. name)
local n = reaper.TrackFX_GetNumParams(tr, fx)
say("num_params=" .. tostring(n))

-- The sample-rate change and the plugin's re-activation happen
-- asynchronously, so give REAPER a moment before reading the reported latency.
local t0 = reaper.time_precise()
local function finish()
  if reaper.time_precise() - t0 < 2.0 then reaper.defer(finish); return end
  local _, dev = reaper.GetAudioDeviceInfo("SRATE", "")
  say("device_srate=" .. tostring(dev))
  local _, lat = reaper.TrackFX_GetNamedConfigParm(tr, fx, "pdc")
  say("pdc=" .. tostring(lat))
  for i = 0, n - 1 do
    local _, pname = reaper.TrackFX_GetParamName(tr, fx, i, "")
    say(string.format("param %s = %.6f", pname, reaper.TrackFX_GetParamNormalized(tr, fx, i)))
  end
  local item = reaper.GetMediaItem(0, 0)
  local len = reaper.GetMediaItemInfo_Value(item, "D_LENGTH")
  reaper.GetSet_LoopTimeRange(true, false, 0, len + 3.0, false)
  reaper.GetSetProjectInfo(0, "RENDER_SRATE", srate, true)
  reaper.GetSetProjectInfo(0, "RENDER_CHANNELS", 2, true)
  reaper.GetSetProjectInfo(0, "RENDER_BOUNDSFLAG", 2, true)
  reaper.GetSetProjectInfo_String(0, "RENDER_FILE", out_dir, true)
  reaper.GetSetProjectInfo_String(0, "RENDER_PATTERN", "reaper_render", true)
  -- WAV, 32-bit float, so the comparison with the offline render is exact.
  reaper.GetSetProjectInfo_String(0, "RENDER_FORMAT", "ZXZhdyAAAQ==", true)
  reaper.Main_OnCommand(42230, 0) -- render with most recent settings, auto-close
  say("render_done=true")
  for i = 0, n - 1 do
    local _, pname = reaper.TrackFX_GetParamName(tr, fx, i, "")
    say(string.format("param after %s = %.6f", pname, reaper.TrackFX_GetParamNormalized(tr, fx, i)))
  end
  log:close()
end
reaper.defer(finish)
