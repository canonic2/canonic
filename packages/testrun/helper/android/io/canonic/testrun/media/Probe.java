package io.canonic.testrun.media;

import android.content.Context;
import android.media.*;
import android.os.SystemClock;
import android.util.Base64;
import org.json.*;
import java.io.ByteArrayOutputStream;
import java.lang.reflect.Method;
import java.util.List;

/** Owns exactly one bounded remote-submix recorder; never changes volume or selects a route. */
final class Probe {
    static final int RATE=48000, CHANNELS=2;
    final Context context;
    final AudioManager audio;
    boolean quarantined;
    Probe(Context context, AudioManager audio) { this.context=context;this.audio=audio; }
    JSONObject routes() {
        try {
            AudioAttributes attributes=new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).build();
            Method query=AudioManager.class.getMethod("getDevicesForAttributes",AudioAttributes.class);
            List<?> devices=(List<?>)query.invoke(audio,attributes);
            JSONArray list=new JSONArray();
            for(Object device:devices) {
                if(list.length()==16) break;
                list.put(Main.obj("type",device.getClass().getMethod("getType").invoke(device),
                    "role",device.getClass().getMethod("getRole").invoke(device)));
            }
            return Main.obj("source","AudioManager.getDevicesForAttributes","devices",list);
        } catch(Exception unavailable) { return Main.obj("source",null,"devices",null,"unavailableReason",Main.bounded(unavailable.toString())); }
    }
    JSONObject capture(JSONObject args, boolean[] dispatched) throws Exception {
        if(quarantined) throw new Main.Refused("AUDIO_PROBE_FAILED","Previous recorder cleanup is unverified; disconnect this controller");
        if(!args.optBoolean("acceptRouteChange")) throw new Main.Refused("INVALID_ARGUMENT","acceptRouteChange:true is required");
        int duration=args.optInt("durationMs",2000);
        if(duration<100 || duration>10000) throw new Main.Refused("INVALID_ARGUMENT","durationMs must be 100..10000");
        boolean retain=args.optBoolean("retainClip",false);
        ByteArrayOutputStream clip=retain ? new ByteArrayOutputStream() : null;
        JSONObject before=routes(),during=null,after=null;
        AudioRecord recorder=null;
        long started=SystemClock.elapsedRealtime(), first=-1,last=-1, count=0,zeros=0,reads=0;
        double squares=0,peak=0;
        String phase="initialization",cause=null,cleanupCause=null;
        boolean recording=false;
        dispatched[0]=true;
        try {
            int minimum=AudioRecord.getMinBufferSize(RATE,AudioFormat.CHANNEL_IN_STEREO,AudioFormat.ENCODING_PCM_16BIT);
            if(minimum<=0 || minimum>1024*1024) throw new IllegalStateException("Unsupported native buffer size: "+minimum);
            AudioRecord.Builder builder=new AudioRecord.Builder();
            AudioRecord.Builder.class.getMethod("setContext",Context.class).invoke(builder,context);
            recorder=builder.setAudioSource(MediaRecorder.AudioSource.REMOTE_SUBMIX)
                .setAudioFormat(new AudioFormat.Builder().setSampleRate(RATE).setChannelMask(AudioFormat.CHANNEL_IN_STEREO).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setBufferSizeInBytes(minimum*4).build();
            if(recorder.getState()!=AudioRecord.STATE_INITIALIZED) throw new IllegalStateException("Recorder is not initialized");
            phase="start"; recorder.startRecording(); recording=true;
            if(recorder.getRecordingState()!=AudioRecord.RECORDSTATE_RECORDING) throw new IllegalStateException("Recorder did not enter recording state");
            during=routes(); phase="read";
            short[] buffer=new short[960]; long wanted=(long)duration*RATE*CHANNELS/1000;
            long deadline=SystemClock.elapsedRealtime()+duration+2000;
            while(count<wanted) {
                long now=SystemClock.elapsedRealtime();
                if(now>deadline || now-(last<0?started:last)>1000) throw new IllegalStateException("No PCM progress within capture deadline");
                int size=(int)Math.min(buffer.length,wanted-count);
                int n=recorder.read(buffer,0,size,AudioRecord.READ_NON_BLOCKING);
                if(n<0) throw new IllegalStateException("Native read error: "+n);
                if(n==0) { SystemClock.sleep(5); continue; }
                if(n%CHANNELS!=0) throw new IllegalStateException("Incomplete PCM frame");
                now=SystemClock.elapsedRealtime(); if(first<0)first=now; last=now; reads++;
                for(int i=0;i<n;i++) {
                    int sample=buffer[i];double v=sample/32768.0;
                    squares+=v*v;peak=Math.max(peak,Math.abs(v));zeros+=sample==0?1:0;
                    if(clip!=null) { clip.write(sample&255);clip.write((sample>>8)&255); }
                }
                count+=n;
            }
        } catch(Exception error) { cause=Main.bounded(error.toString()); }
        finally {
            if(recorder!=null) {
                try { if(recording)recorder.stop(); } catch(Exception error) {cleanupCause=Main.bounded(error.toString());}
                try {recorder.release();} catch(Exception error) {cleanupCause=Main.bounded(error.toString());}
            }
            after=routes();
        }
        boolean cleanup=cleanupCause==null; quarantined=!cleanup;
        long finished=SystemClock.elapsedRealtime();
        JSONObject data=Main.obj("backend","android_remote_submix","sampleRate",RATE,"channels",CHANNELS,
            "sampleCount",count,"frameCount",count/CHANNELS,"decodedDurationMs",count/(double)(RATE*CHANNELS)*1000,
            "digitalZeroSampleCount",zeros,"digitalZero",count>0&&zeros==count,
            "rmsDbfs",squares>0?10*Math.log10(squares/count):null,"peakDbfs",peak>0?20*Math.log10(peak):null,
            "wallDurationMs",finished-started,"firstPacketAgeMs",first<0?null:finished-first,"lastPacketAgeMs",last<0?null:finished-last,
            "health",Main.obj("reads",reads,"packetLoss",null,"packetLossUnavailableReason","PCM AudioRecord exposes no RTP sequence","decodeErrors",0,"captureHealthy",cause==null),
            "routes",Main.obj("before",before,"during",during,"after",after,"restoration",before.isNull("devices")||after.isNull("devices")?"unavailable":before.toString().equals(after.toString())?"observed_same":"observed_changed"),
            "cleanupVerified",cleanup,"observed",count>0,"experimental",true,"attribution","system_mix",
            "volumeStage","pre_output_volume","audibility","unverified","integrity","not_fully_verifiable",
            "limitations","Remote submix changes routing and captured policy-restricted fixture audio on API 31; this does not prove app identity or audibility");
        if(cause!=null || !cleanup) throw new Failure(phase,cause,cleanupCause,data);
        if(clip!=null) data.put("pcmBase64",Base64.encodeToString(clip.toByteArray(),Base64.NO_WRAP));
        return data;
    }
    static final class Failure extends Exception {
        final JSONObject details;
        Failure(String phase,String cause,String cleanup,JSONObject partial) {
            super(cause==null?"Recorder cleanup failed":cause);
            details=Main.obj("phase",phase,"cause",cause,"cleanupCause",cleanup,"cleanupVerified",cleanup==null,"partial",partial);
        }
    }
}
