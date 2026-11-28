package io.canonic.testrun.capture;

import android.media.MediaCodec;
import android.media.MediaFormat;
import android.os.IBinder;
import android.os.Process;
import android.os.SystemClock;
import android.view.Surface;
import android.graphics.Rect;
import java.io.BufferedReader;
import java.io.DataOutputStream;
import java.io.InputStreamReader;
import java.io.RandomAccessFile;
import java.nio.ByteBuffer;
import java.nio.channels.FileLock;
import java.util.concurrent.ConcurrentSkipListMap;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

/** Shell-owned non-secure source. Complete units carry source acquisition identity. */
public final class Main {
    final AtomicBoolean stopped = new AtomicBoolean();
    final AtomicLong heartbeat = new AtomicLong(SystemClock.elapsedRealtime());
    final ConcurrentSkipListMap<Long,long[]> acquisitions = new ConcurrentSkipListMap<>();
    final DataOutputStream output = new DataOutputStream(System.out);
    final String nonce;
    Renderer renderer;
    Main(String nonce) { this.nonce = nonce; }

    synchronized void packet(int type, long request, long sequence, byte[] payload) throws Exception {
        output.writeInt(0x54525631); output.writeInt(type); output.writeInt(payload.length);
        output.writeInt(0); output.writeLong(request); output.writeLong(sequence);
        output.write(payload); output.flush();
    }

    void controls() {
        try {
            BufferedReader reader = new BufferedReader(new InputStreamReader(System.in, "UTF-8"));
            String line;
            while ((line = reader.readLine()) != null && !stopped.get()) {
                if (line.equals("S")) break;
                if (line.equals("P")) { heartbeat.set(SystemClock.elapsedRealtime()); continue; }
                if (line.startsWith("B ") && line.length() <= 24) {
                    final long id = Long.parseLong(line.substring(2));
                    if (id <= 0 || renderer == null) break;
                    renderer.barrier(id);
                } else break;
            }
        } catch (Exception unavailable) { /* EOF/protocol failure revokes the source. */ }
        finally { stopped.set(true); }
    }

    public static void main(String[] args) throws Exception {
        if (Process.myUid() != 2000 || android.os.Build.VERSION.SDK_INT != 31 || args.length != 3)
            throw new SecurityException("Unsupported capture platform");
        int width = Integer.parseInt(args[1]), height = Integer.parseInt(args[2]);
        if (width < 2 || height < 2 || width > 8192 || height > 8192
                || (long)width*height > 16777216 || (width & 1) != 0 || (height & 1) != 0)
            throw new IllegalArgumentException("Unsupported capture geometry");
        // Keep this inode in place: unlinking a locked file would allow another
        // producer to lock a replacement inode. Process exit releases the lock.
        try (RandomAccessFile owner = new RandomAccessFile("/data/local/tmp/testrun-capture.lock", "rw");
             FileLock lock = owner.getChannel().tryLock()) {
            if (lock == null) throw new IllegalStateException("Android capture already owned");
            new Main(args[0]).run(width,height);
        }
    }

    void run(int width, int height) throws Exception {
        Thread watchdog = new Thread(new Runnable() {
            @Override public void run() {
            long stopping = 0;
            for (;;) {
                long now = SystemClock.elapsedRealtime();
                if (now-heartbeat.get() >= 5000) stopped.set(true);
                if (stopped.get()) {
                    if (stopping == 0) stopping = now;
                    if (now-stopping >= 3000) Process.killProcess(Process.myPid());
                }
                SystemClock.sleep(100);
            }
            }
        },"TestrunCaptureLease");
        watchdog.setDaemon(true); watchdog.start();
        MediaCodec codec = null;
        Surface encoded = null;
        IBinder display = null;
        Class<?> control = Class.forName("android.view.SurfaceControl");
        try {
            codec = MediaCodec.createEncoderByType("video/avc");
            MediaFormat format = MediaFormat.createVideoFormat("video/avc",width,height);
            format.setInteger(MediaFormat.KEY_COLOR_FORMAT,0x7f000789);
            format.setInteger(MediaFormat.KEY_BIT_RATE,8000000);
            format.setInteger(MediaFormat.KEY_FRAME_RATE,60);
            format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL,2);
            // No encoder repeats: only Renderer acquisitions obtain source identity.
            codec.configure(format,null,null,MediaCodec.CONFIGURE_FLAG_ENCODE);
            encoded = codec.createInputSurface(); codec.start();
            renderer = new Renderer(this,encoded,width,height);
            renderer.open();
            display = (IBinder)control.getDeclaredMethod("createDisplay",String.class,boolean.class)
                    .invoke(null,"Testrun capture",false);
            control.getDeclaredMethod("openTransaction").invoke(null);
            try {
                control.getDeclaredMethod("setDisplaySurface",IBinder.class,Surface.class)
                        .invoke(null,display,renderer.surface);
                control.getDeclaredMethod("setDisplayProjection",IBinder.class,int.class,Rect.class,Rect.class)
                        .invoke(null,display,0,new Rect(0,0,width,height),new Rect(0,0,width,height));
                control.getDeclaredMethod("setDisplayLayerStack",IBinder.class,int.class)
                        .invoke(null,display,0);
            } finally { control.getDeclaredMethod("closeTransaction").invoke(null); }
            byte[] hello = ("{\"version\":1,\"nonce\":\""+nonce+"\",\"width\":"+width
                    +",\"height\":"+height+",\"sourceFence\":\"surface_acquisition\"}").getBytes("UTF-8");
            packet(4,0,0,hello);
            Thread input = new Thread(new Runnable() {
                @Override public void run() { controls(); }
            },"TestrunCaptureControl"); input.setDaemon(true); input.start();
            MediaCodec.BufferInfo info = new MediaCodec.BufferInfo();
            while (!stopped.get() && SystemClock.elapsedRealtime()-heartbeat.get() < 5000) {
                int index = codec.dequeueOutputBuffer(info,100000);
                if (index < 0) continue;
                try {
                    if (info.size <= 0) continue;
                    if (info.size > 8*1024*1024) throw new IllegalStateException("Capture packet limit");
                    ByteBuffer buffer = codec.getOutputBuffer(index);
                    buffer.position(info.offset); buffer.limit(info.offset+info.size);
                    byte[] bytes = new byte[info.size]; buffer.get(bytes);
                    if ((info.flags & MediaCodec.BUFFER_FLAG_CODEC_CONFIG) != 0) packet(1,0,0,bytes);
                    else {
                        long[] acquisition = acquisitions.remove(info.presentationTimeUs);
                        acquisitions.headMap(info.presentationTimeUs).clear();
                        if (acquisition != null) packet(2,acquisition[0],acquisition[1],bytes);
                    }
                } finally { codec.releaseOutputBuffer(index,false); }
            }
        } finally {
            stopped.set(true);
            try { if (display != null) control.getDeclaredMethod("destroyDisplay",IBinder.class).invoke(null,display); }
            finally {
                try { if (renderer != null) renderer.close(); }
                finally {
                    try { if (codec != null) { try { codec.stop(); } finally { codec.release(); } } }
                    finally { if (encoded != null) encoded.release(); }
                }
            }
        }
    }
}
