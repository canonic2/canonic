package io.canonic.testrun.media;

import android.content.Context;
import android.content.pm.PackageManager;
import android.media.AudioManager;
import android.media.MediaMetadata;
import android.media.VolumeProvider;
import android.media.session.*;
import android.os.*;
import org.json.*;
import java.io.*;
import java.lang.reflect.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

/** Versioned, bounded JSON-line media adapter. Stdout is protocol only. */
public final class Main {
    static final int WIRE = 1;
    static final PrintStream OUT = new PrintStream(new FileOutputStream(FileDescriptor.out), true);
    final MediaSessionManager sessions;
    final AudioManager audio;
    final Probe probe;
    final String epoch = UUID.randomUUID().toString();
    final Map<MediaSession.Token, String> refs = new HashMap<>();
    final Set<String> truncated = new LinkedHashSet<>();
    Main(Context context) {
        if (android.os.Process.myUid() != 2000 || context.checkSelfPermission("android.permission.MEDIA_CONTENT_CONTROL") != PackageManager.PERMISSION_GRANTED)
            throw new SecurityException("Shell media-control permission required");
        sessions = (MediaSessionManager) context.getSystemService(Context.MEDIA_SESSION_SERVICE);
        audio = (AudioManager) context.getSystemService(Context.AUDIO_SERVICE);
        probe = new Probe(context,audio);
        sessions.getActiveSessions(null); audio.getStreamVolume(AudioManager.STREAM_MUSIC);
    }
    public static void main(String[] args) {
        // Prevent framework chatter from becoming protocol frames.
        System.setOut(System.err);
        // One-time package read: prints one frame and exits, without media services.
        if (args.length == 1 && args[0].equals("apps")) {
            try { emit(obj("version", WIRE, "ok", true, "data", Apps.list(Shell.create()))); }
            catch (Exception e) { emit(obj("version", WIRE, "ok", false, "error", bounded(e.toString()))); }
            System.exit(0);
        }
        try {
            Main main = new Main(Shell.create());
            emit(obj("version", WIRE, "ready", true, "pid", android.os.Process.myPid(), "uid", android.os.Process.myUid(), "sdk", Build.VERSION.SDK_INT, "helperEpoch", main.epoch));
            InputStreamReader in = new InputStreamReader(System.in, StandardCharsets.UTF_8);
            String line;
            while ((line = line(in)) != null) {
                JSONObject request = new JSONObject(line);
                Object id = request.get("id");
                if (request.getInt("version") != WIRE) throw new IOException("Protocol version mismatch");
                if ("quit".equals(request.getString("operation"))) {
                    emit(obj("version", WIRE, "id", id, "ok", true, "data", obj("closed", true))); break;
                }
                boolean[] dispatched = {false};
                try {
                    JSONObject data = main.call(request, dispatched);
                    emit(obj("version", WIRE, "id", id, "ok", true, "data", data));
                } catch (Exception e) {
                    if(e instanceof Probe.Failure) {
                        emit(obj("version",WIRE,"id",id,"ok",false,"error",obj("code","AUDIO_PROBE_FAILED","message",bounded(e.getMessage()),"dispatched",true,"details",((Probe.Failure)e).details)));
                        continue;
                    }
                    String code = e instanceof Refused ? ((Refused)e).code : "MEDIA_HELPER_FAILED";
                    emit(obj("version", WIRE, "id", id, "ok", false, "error", obj("code", dispatched[0] ? "MEDIA_OUTCOME_UNKNOWN" : code,
                        "message", bounded(e.getMessage()), "dispatched", dispatched[0])));
                }
            }
        } catch (Exception e) {
            emit(obj("version", WIRE, "ready", false, "code", e instanceof UnsupportedOperationException || e instanceof SecurityException ? "MEDIA_UNSUPPORTED" : "MEDIA_HELPER_FAILED", "error", bounded(e.toString())));
        }
        // app_process has framework threads that otherwise outlive stdin EOF.
        System.exit(0);
    }
    static String line(Reader in) throws IOException {
        StringBuilder b = new StringBuilder(); int c;
        while ((c = in.read()) != -1 && c != '\n') {
            if (b.length() >= 1024 * 1024) throw new IOException("Request exceeds limit");
            b.append((char)c);
        }
        return c == -1 && b.length() == 0 ? null : b.toString();
    }
    static JSONObject obj(Object... pairs) {
        JSONObject result = new JSONObject();
        try { for (int i=0; i<pairs.length; i+=2) result.put((String)pairs[i], pairs[i+1] == null ? JSONObject.NULL : pairs[i+1]); }
        catch (JSONException impossible) { throw new IllegalArgumentException(impossible); }
        return result;
    }
    static void emit(JSONObject value) {
        String text = value.toString();
        if (text.getBytes(StandardCharsets.UTF_8).length > 4*1024*1024) throw new IllegalStateException("Response exceeds limit");
        OUT.println(text);
    }
    static String bounded(String text) {
        if (text == null) return null;
        int count = text.codePointCount(0, text.length());
        return count > 512 ? text.substring(0, text.offsetByCodePoints(0, 512)) : text;
    }
    Object text(String field, CharSequence value) {
        if (value == null) return JSONObject.NULL;
        String original = value.toString(), result = bounded(original);
        if (!original.equals(result)) truncate(field);
        return result;
    }
    void truncate(String field) {
        if(truncated.size()>=64) return;
        if(field.codePointCount(0,field.length())>128) field=field.substring(0,field.offsetByCodePoints(0,128));
        truncated.add(field);
    }
    List<MediaController> active() {
        List<MediaController> active = sessions.getActiveSessions(null);
        Set<MediaSession.Token> live = new HashSet<>();
        for (MediaController c : active) live.add(c.getSessionToken());
        refs.keySet().retainAll(live);
        return active;
    }
    String ref(MediaController c) {
        MediaSession.Token token=c.getSessionToken();
        String value=refs.get(token);
        if(value==null) {value=epoch+":"+UUID.randomUUID(); refs.put(token,value);}
        return value;
    }
    MediaController resolve(String reference) throws Exception {
        for (MediaController c : active()) if (reference.equals(refs.get(c.getSessionToken()))) return c;
        throw new Refused("MEDIA_SESSION_STALE", "Read media state and select a current sessionRef");
    }
    JSONObject snapshot() throws Exception {
        long start=SystemClock.elapsedRealtime();
        truncated.clear(); List<MediaController> active = active(); JSONArray list = new JSONArray(); int bytes=0;
        for (MediaController c : active) {
            if (list.length() == 32) { truncate("sessions"); break; }
            JSONObject session = describe(c); int size = session.toString().getBytes(StandardCharsets.UTF_8).length;
            if (bytes + size > 768*1024) { truncate("snapshot"); break; }
            bytes += size; list.put(session);
        }
        return obj("source", "android_media_controller", "helperEpoch", epoch, "count", active.size(), "sessions", list,
            "partial", !truncated.isEmpty(), "truncationReasons", new JSONArray(truncated), "readStartElapsedMs",start,"readEndElapsedMs",SystemClock.elapsedRealtime(),"observedElapsedMs", SystemClock.elapsedRealtime());
    }
    JSONObject describe(MediaController c) throws Exception {
        String reference = ref(c); long readStart=SystemClock.elapsedRealtime();
        PlaybackState s = c.getPlaybackState(); long readEnd=SystemClock.elapsedRealtime();
        MediaMetadata m = c.getMetadata();
        JSONArray unavailable = new JSONArray(); JSONObject metadata = new JSONObject();
        for (String key : new String[]{"title", "artist", "album", "durationMs"}) metadata.put(key, JSONObject.NULL);
        if (m != null) {
            metadata.put("title", text(reference+".title", m.getText(MediaMetadata.METADATA_KEY_TITLE)));
            metadata.put("artist", text(reference+".artist", m.getText(MediaMetadata.METADATA_KEY_ARTIST)));
            metadata.put("album", text(reference+".album", m.getText(MediaMetadata.METADATA_KEY_ALBUM)));
            metadata.put("durationMs", m.containsKey(MediaMetadata.METADATA_KEY_DURATION) ? m.getLong(MediaMetadata.METADATA_KEY_DURATION) : JSONObject.NULL);
            JSONObject nativeValues = new JSONObject();
            try {
                Field f = MediaMetadata.class.getDeclaredField("mBundle"); f.setAccessible(true); Bundle b = (Bundle)f.get(m); int n=0;
                for (String key : new TreeSet<>(m.keySet())) {
                    if (n++ >= 48) { truncate(reference+".metadata"); break; }
                    Object value = b.get(key);
                    if (key.codePointCount(0,key.length()) > 512) { truncate(reference+".metadataKey"); continue; }
                    if (value instanceof CharSequence) nativeValues.put(key, text(reference+"."+key, (CharSequence)value));
                    else if (value instanceof Long || value instanceof Integer || value instanceof Boolean) nativeValues.put(key, value);
                }
            } catch (ReflectiveOperationException e) { unavailable.put("metadata.nativeValues"); }
            metadata.put("nativeValues", nativeValues);
        } else unavailable.put("metadata");
        Object tag = JSONObject.NULL, uid = JSONObject.NULL;
        try { tag = text(reference+".tag", (String)MediaController.class.getMethod("getTag").invoke(c)); }
        catch (ReflectiveOperationException e) { unavailable.put("tag"); }
        try { uid = MediaSession.Token.class.getMethod("getUid").invoke(c.getSessionToken()); }
        catch (ReflectiveOperationException e) { unavailable.put("tokenUid"); }
        long actions = s == null ? 0 : s.getActions();
        JSONObject state = obj("nativeState", s == null ? null : s.getState(), "nativeStateName", s == null ? null : stateName(s.getState()), "state", s == null ? "unknown" : stateName(s.getState()),
            "nativePositionMs", s == null ? null : s.getPosition(), "positionMs", s == null || s.getPosition() < 0 ? null : s.getPosition(),
            "positionSource", "service_extrapolated", "appReportedPositionMs", null, "speed", s == null ? null : s.getPlaybackSpeed(),
            "clock", "android_elapsed_realtime", "updatedAtElapsedMs", s == null ? null : s.getLastPositionUpdateTime(),
            "readStartElapsedMs",readStart,"readEndElapsedMs",readEnd,"observedElapsedMs", readEnd, "nativeActions", Long.toString(actions),
            "errorMessage", s == null ? null : text(reference+".errorMessage", s.getErrorMessage()));
        if (s == null) unavailable.put("reportedPlayback");
        JSONArray commands = new JSONArray();
        for (String cmd : new String[]{"play", "pause", "play_pause", "next", "previous", "stop"}) {
            if(cmd.equals("play_pause")) {
                int code=s==null?-1:s.getState();
                if(code==PlaybackState.STATE_PLAYING && supports(actions,"pause")
                    || (code==PlaybackState.STATE_PAUSED || code==PlaybackState.STATE_STOPPED) && supports(actions,"play")) commands.put(cmd);
            } else if (supports(actions, cmd)) commands.put(cmd);
        }
        JSONObject result = obj("sessionRef", reference, "packageName", text(reference+".packageName", c.getPackageName()), "tag", tag, "tokenUid", uid,
            "mediaId", m == null ? null : text(reference+".mediaId", m.getText(MediaMetadata.METADATA_KEY_MEDIA_ID)),
            "reportedPlayback", state, "metadata", metadata, "supportedCommands", commands, "volume", sessionVolume(c), "unavailableFields", unavailable);
        JSONObject nativeValues=metadata.optJSONObject("nativeValues");
        if (nativeValues!=null) {
            List<String> keys=new ArrayList<>(); Iterator<String> iterator=nativeValues.keys();
            while(iterator.hasNext()) keys.add(iterator.next());
            Collections.sort(keys);
            while(result.toString().getBytes(StandardCharsets.UTF_8).length>32*1024 && !keys.isEmpty()) {
                nativeValues.remove(keys.remove(keys.size()-1)); truncate(reference+".metadata");
            }
        }
        JSONObject reasons=new JSONObject();
        for(int i=0;i<unavailable.length();i++) reasons.put(unavailable.getString(i),"Not published or not exposed by this native API");
        for(String key:new String[]{"title","artist","album","durationMs"})
            if(metadata.isNull(key)) reasons.put("metadata."+key,"Not published by this media session");
        if(result.isNull("mediaId")) reasons.put("mediaId","Not published by this media session");
        if(result.isNull("tag")) reasons.put("tag","Not published or not exposed by this native API");
        if(result.isNull("tokenUid")) reasons.put("tokenUid","Not exposed by this native API");
        result.put("unavailableReasons",reasons);
        if(nativeValues!=null) {
            while(result.toString().getBytes(StandardCharsets.UTF_8).length>32*1024 && nativeValues.length()>0) {
                nativeValues.remove(nativeValues.keys().next()); truncate(reference+".metadata");
            }
        }
        return result;
    }
    static String stateName(int state) {
        String[] names={"none","stopped","paused","playing","fast_forwarding","rewinding","buffering","error","connecting","skipping_to_previous","skipping_to_next","skipping_to_queue_item"};
        return state>=0 && state<names.length ? names[state] : "unknown";
    }
    static boolean supports(long actions, String command) {
        long bit;
        switch(command) {
            case "play": bit=PlaybackState.ACTION_PLAY; break; case "pause": bit=PlaybackState.ACTION_PAUSE; break;
            case "stop": bit=PlaybackState.ACTION_STOP; break; case "next": bit=PlaybackState.ACTION_SKIP_TO_NEXT; break;
            case "previous": bit=PlaybackState.ACTION_SKIP_TO_PREVIOUS; break;
            case "play_pause": return supports(actions,"play") && supports(actions,"pause");
            default: return false;
        }
        return (actions & bit) != 0;
    }
    JSONObject sessionVolume(MediaController c) {
        MediaController.PlaybackInfo p=c.getPlaybackInfo();
        if (p==null) return obj("controlType","unknown","min",null,"max",null,"current",null,"muted",null,"fixed",null);
        String control = p.getVolumeControl()==VolumeProvider.VOLUME_CONTROL_ABSOLUTE ? "absolute" : p.getVolumeControl()==VolumeProvider.VOLUME_CONTROL_RELATIVE ? "relative" : "fixed";
        return obj("playbackType",p.getPlaybackType()==MediaController.PlaybackInfo.PLAYBACK_TYPE_REMOTE ? "remote" : "local",
            "controlType",control,"min",0,"minimumSource","platform_defined","max",p.getMaxVolume(),"current",p.getCurrentVolume(),"muted",null,"fixed",control.equals("fixed"),"route",null);
    }
    JSONObject volume(JSONObject target, MediaController c) throws Exception {
        if (c != null) return sessionVolume(c).put("volumeTarget",target).put("source","android_media_controller").put("helperEpoch",epoch).put("observedElapsedMs",SystemClock.elapsedRealtime()).put("unavailableFields",new JSONArray(Arrays.asList("muted","route")));
        int stream=AudioManager.STREAM_MUSIC;
        return obj("source","android_audio_manager","helperEpoch",epoch,"volumeTarget",target,"controlType",audio.isVolumeFixed()?"fixed":"absolute","min",audio.getStreamMinVolume(stream),
            "max",audio.getStreamMaxVolume(stream),"current",audio.getStreamVolume(stream),"muted",audio.isStreamMute(stream),"fixed",audio.isVolumeFixed(),"route",null,"observedElapsedMs",SystemClock.elapsedRealtime(),"unavailableFields",new JSONArray(Arrays.asList("route")));
    }
    JSONObject call(JSONObject request, boolean[] dispatched) throws Exception {
        String op=request.getString("operation"); JSONObject a=request.getJSONObject("arguments");
        if (op.equals("media_state")) return snapshot();
        if (op.equals("audio_probe")) return probe.capture(a,dispatched).put("helperEpoch",epoch);
        MediaController c=null; JSONObject target=a.optJSONObject("volumeTarget");
        if (op.equals("media_control")) c=resolve(a.getString("sessionRef"));
        else if (target != null && target.getString("type").equals("media_session")) c=resolve(target.getString("sessionRef"));
        if (op.equals("volume_get")) return volume(target,c);
        JSONObject before; String command=a.optString("command");
        if (op.equals("media_control")) {
            before=describe(c); PlaybackState state=c.getPlaybackState();
            if (command.equals("play_pause")) {
                int s=state==null ? -1 : state.getState();
                if (s==PlaybackState.STATE_PLAYING) command="pause";
                else if (s==PlaybackState.STATE_PAUSED || s==PlaybackState.STATE_STOPPED) command="play";
                else throw new Refused("MEDIA_UNSUPPORTED","Toggle requires freshly playing, paused or stopped state");
            }
            if (!supports(state==null ? 0 : state.getActions(),command)) throw new Refused("MEDIA_UNSUPPORTED","Session does not advertise this command");
        } else if (op.equals("volume_set")) {
            before=volume(target,c); String mode=before.getString("controlType");
            if (mode.equals("fixed") || mode.equals("unknown")) throw new Refused("MEDIA_UNSUPPORTED","Volume target cannot be changed");
            if (a.has("value") && (!mode.equals("absolute") || a.getInt("value")<before.getInt("min") || a.getInt("value")>before.getInt("max")))
                throw new Refused("INVALID_ARGUMENT","value must be in the current native range of an absolute volume target");
        } else throw new Refused("INVALID_ARGUMENT","Unknown media operation");
        if (request.optBoolean("prepare")) return obj("before",before,"resolvedCommand",command);
        dispatched[0]=true;
        if (op.equals("media_control")) {
            MediaController.TransportControls controls=c.getTransportControls();
            switch(command) {
                case "play": controls.play(); break; case "pause": controls.pause(); break; case "stop": controls.stop(); break;
                case "next": controls.skipToNext(); break; case "previous": controls.skipToPrevious(); break;
                default: throw new IllegalArgumentException("Unresolved media command");
            }
        } else if (a.has("value")) {
            if(c==null) audio.setStreamVolume(AudioManager.STREAM_MUSIC,a.getInt("value"),0); else c.setVolumeTo(a.getInt("value"),0);
        } else {
            int direction=a.getString("adjust").equals("up") ? AudioManager.ADJUST_RAISE : AudioManager.ADJUST_LOWER;
            if(c==null) audio.adjustStreamVolume(AudioManager.STREAM_MUSIC,direction,0); else c.adjustVolume(direction,0);
        }
        JSONObject effect=obj("state","not_checked"); int window=a.optInt("observeForMs",0);
        if(window>0) {
            long start=SystemClock.elapsedRealtime(), end=start+window; JSONObject after=before;
            try {
                do {
                    if(c!=null) c=resolve(a.optString("sessionRef",target==null ? "" : target.optString("sessionRef")));
                    after=op.equals("media_control") ? describe(c) : volume(target,c);
                    boolean done;
                    if (op.equals("media_control") && (command.equals("play") || command.equals("pause")))
                        done=(command.equals("play")?"playing":"paused").equals(after.getJSONObject("reportedPlayback").getString("state"));
                    else if (a.has("value")) done=after.getInt("current")==a.getInt("value");
                    else if (op.equals("media_control") && (command.equals("next") || command.equals("previous")))
                        done=skipped(before,after,command) && settled(before,after);
                    else done=!observation(before,op).equals(observation(after,op));
                    if (done) break;
                    SystemClock.sleep(Math.min(25,Math.max(0,end-SystemClock.elapsedRealtime())));
                } while(SystemClock.elapsedRealtime()<end);
                boolean changed=!observation(before,op).equals(observation(after,op));
                // A restart keeps the track and state; only its position says it happened.
                if (op.equals("media_control") && command.equals("previous")) changed|=skipped(before,after,command);
                effect=obj("state",changed?"state_changed":"no_change","before",before,"after",after,"durationMs",SystemClock.elapsedRealtime()-start);
                if (op.equals("media_control") && (command.equals("play") || command.equals("pause"))) {
                    String desired=command.equals("play")?"playing":"paused";
                    effect.put("already_satisfied",desired.equals(before.getJSONObject("reportedPlayback").getString("state")));
                    effect.put("predicateEstablished",desired.equals(after.getJSONObject("reportedPlayback").getString("state")));
                } else if (op.equals("media_control") && (command.equals("next") || command.equals("previous"))) {
                    effect.put("predicateEstablished",skipped(before,after,command) && settled(before,after));
                } else if (a.has("value")) {
                    effect.put("already_satisfied",before.getInt("current")==a.getInt("value"));
                    effect.put("predicateEstablished",after.getInt("current")==a.getInt("value"));
                }
            } catch (Exception unavailable) { effect=obj("state","unavailable","reason",bounded(unavailable.getMessage())); }
        }
        return obj("source","android_media_controller","helperEpoch",epoch,"dispatch",obj("state","sent","command",command,"destination",op.equals("media_control")?a.getString("sessionRef"):target,"deliveryAcknowledged",false),
            "effect",effect,"observed",effect.optBoolean("predicateEstablished",false),"audibility","unverified");
    }
    /** Another track, or for previous the same track restarted from an earlier position. */
    static boolean skipped(JSONObject before, JSONObject after, String command) throws Exception {
        String track=obj("metadata",before.get("metadata"),"mediaId",before.get("mediaId")).toString();
        if (!track.equals(obj("metadata",after.get("metadata"),"mediaId",after.get("mediaId")).toString())) return true;
        if (!command.equals("previous")) return false;
        JSONObject from=before.getJSONObject("reportedPlayback"), to=after.getJSONObject("reportedPlayback");
        return !from.isNull("positionMs") && !to.isNull("positionMs") && to.getLong("positionMs")+1000<from.getLong("positionMs");
    }
    /** The player left its loading states and, if it named the old track, names the new one. */
    static boolean settled(JSONObject before, JSONObject after) throws Exception {
        String state=after.getJSONObject("reportedPlayback").getString("state");
        if (Arrays.asList("none","unknown","buffering","connecting","skipping_to_previous","skipping_to_next","skipping_to_queue_item").contains(state)) return false;
        return before.getJSONObject("metadata").isNull("title") || !after.getJSONObject("metadata").isNull("title");
    }
    static String observation(JSONObject value, String op) throws Exception {
        if (!op.equals("media_control")) return obj("current",value.get("current"),"muted",value.get("muted"),"fixed",value.get("fixed"),"controlType",value.get("controlType")).toString();
        return obj("state",value.getJSONObject("reportedPlayback").get("state"),"metadata",value.get("metadata"),"mediaId",value.get("mediaId")).toString();
    }
    static final class Refused extends Exception {
        final String code;
        Refused(String code,String message) { super(message); this.code=code; }
    }
}
