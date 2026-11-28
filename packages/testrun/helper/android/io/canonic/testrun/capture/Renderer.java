package io.canonic.testrun.capture;

import android.graphics.SurfaceTexture;
import android.opengl.*;
import android.os.Handler;
import android.os.HandlerThread;
import android.view.Surface;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.FloatBuffer;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

/** Serial source acquisition and barriers on the same GL queue, independent of encoder output. */
final class Renderer {
    final Main owner;
    final Surface encoded;
    final int width,height;
    final HandlerThread thread = new HandlerThread("TestrunCaptureSurface");
    Handler handler;
    Surface surface;
    SurfaceTexture texture;
    EGLDisplay display;
    EGLContext context;
    EGLSurface window;
    int name,program;
    long request,sequence,barrierTimestamp,lastTimestamp;
    final float[] matrix=new float[16];
    final FloatBuffer vertices=ByteBuffer.allocateDirect(16*4).order(ByteOrder.nativeOrder()).asFloatBuffer();
    volatile Throwable failure;
    Renderer(Main owner,Surface encoded,int width,int height) {
        this.owner=owner; this.encoded=encoded; this.width=width; this.height=height;
        vertices.put(new float[]{-1,-1,0,0, 1,-1,1,0, -1,1,0,1, 1,1,1,1}).position(0);
    }
    void open() throws Exception {
        thread.start(); handler=new Handler(thread.getLooper());
        CountDownLatch ready=new CountDownLatch(1);
        handler.post(new Runnable() {
            @Override public void run() { try { initialize(); } catch(Throwable error) { failure=error; } finally { ready.countDown(); } }
        });
        if (!ready.await(2,TimeUnit.SECONDS) || failure != null) throw new IllegalStateException("Surface setup failed",failure);
    }
    void barrier(final long next) {
        handler.post(new Runnable() {
            @Override public void run() {
            try {
                // Drain pre-request queued buffers. A callback already queued before this
                // barrier cannot qualify unless it acquires a distinct later source buffer.
                texture.updateTexImage();
                barrierTimestamp=texture.getTimestamp();
                request=next;
                owner.packet(3,next,sequence,new byte[0]);
            } catch(Exception error) { failure=error; owner.stopped.set(true); }
            }
        });
    }
    void initialize() {
        display=EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY);
        if (!EGL14.eglInitialize(display,new int[2],0,new int[2],0)) throw new IllegalStateException();
        EGLConfig[] configs=new EGLConfig[1]; int[] count=new int[1];
        int[] attrs={EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,
                EGL14.EGL_RENDERABLE_TYPE,EGL14.EGL_OPENGL_ES2_BIT,0x3142,1,EGL14.EGL_NONE};
        if (!EGL14.eglChooseConfig(display,attrs,0,configs,0,1,count,0) || count[0]!=1) throw new IllegalStateException();
        context=EGL14.eglCreateContext(display,configs[0],EGL14.EGL_NO_CONTEXT,
                new int[]{EGL14.EGL_CONTEXT_CLIENT_VERSION,2,EGL14.EGL_NONE},0);
        window=EGL14.eglCreateWindowSurface(display,configs[0],encoded,new int[]{EGL14.EGL_NONE},0);
        if (!EGL14.eglMakeCurrent(display,window,window,context)) throw new IllegalStateException();
        int[] textureName=new int[1]; GLES20.glGenTextures(1,textureName,0); name=textureName[0];
        GLES20.glBindTexture(GLES11Ext.GL_TEXTURE_EXTERNAL_OES,name);
        GLES20.glTexParameteri(GLES11Ext.GL_TEXTURE_EXTERNAL_OES,GLES20.GL_TEXTURE_MIN_FILTER,GLES20.GL_LINEAR);
        GLES20.glTexParameteri(GLES11Ext.GL_TEXTURE_EXTERNAL_OES,GLES20.GL_TEXTURE_MAG_FILTER,GLES20.GL_LINEAR);
        GLES20.glTexParameteri(GLES11Ext.GL_TEXTURE_EXTERNAL_OES,GLES20.GL_TEXTURE_WRAP_S,GLES20.GL_CLAMP_TO_EDGE);
        GLES20.glTexParameteri(GLES11Ext.GL_TEXTURE_EXTERNAL_OES,GLES20.GL_TEXTURE_WRAP_T,GLES20.GL_CLAMP_TO_EDGE);
        program=GLES20.glCreateProgram();
        GLES20.glAttachShader(program,shader(GLES20.GL_VERTEX_SHADER,
                "attribute vec2 p; attribute vec2 t; uniform mat4 m; varying vec2 v; void main(){gl_Position=vec4(p,0.,1.);v=(m*vec4(t,0.,1.)).xy;}"));
        GLES20.glAttachShader(program,shader(GLES20.GL_FRAGMENT_SHADER,
                "#extension GL_OES_EGL_image_external : require\nprecision mediump float;uniform samplerExternalOES s;varying vec2 v;void main(){gl_FragColor=texture2D(s,v);}"));
        GLES20.glLinkProgram(program);
        int[] linked=new int[1]; GLES20.glGetProgramiv(program,GLES20.GL_LINK_STATUS,linked,0);
        if (linked[0]==0) throw new IllegalStateException("Shader link failed");
        texture=new SurfaceTexture(name); texture.setDefaultBufferSize(width,height);
        texture.setOnFrameAvailableListener(new SurfaceTexture.OnFrameAvailableListener() {
            @Override public void onFrameAvailable(SurfaceTexture ignored) { acquire(); }
        },handler);
        surface=new Surface(texture);
    }
    static int shader(int kind,String source) {
        int shader=GLES20.glCreateShader(kind); GLES20.glShaderSource(shader,source); GLES20.glCompileShader(shader);
        int[] ok=new int[1]; GLES20.glGetShaderiv(shader,GLES20.GL_COMPILE_STATUS,ok,0);
        if(ok[0]==0) throw new IllegalStateException("Shader compilation failed"); return shader;
    }
    void acquire() {
        if(owner.stopped.get()) return;
        try {
            texture.updateTexImage();
            long timestamp=texture.getTimestamp();
            if(timestamp<=barrierTimestamp || timestamp<=lastTimestamp) return;
            lastTimestamp=timestamp;
            // The source is acquired here, on the same queue as barrier acknowledgement.
            long seq=++sequence, pts=System.nanoTime()/1000;
            owner.acquisitions.put(pts,new long[]{request,seq});
            while(owner.acquisitions.size()>128) owner.acquisitions.pollFirstEntry();
            texture.getTransformMatrix(matrix);
            GLES20.glViewport(0,0,width,height); GLES20.glUseProgram(program);
            int position=GLES20.glGetAttribLocation(program,"p"), coord=GLES20.glGetAttribLocation(program,"t");
            vertices.position(0); GLES20.glVertexAttribPointer(position,2,GLES20.GL_FLOAT,false,16,vertices);
            vertices.position(2); GLES20.glVertexAttribPointer(coord,2,GLES20.GL_FLOAT,false,16,vertices);
            GLES20.glEnableVertexAttribArray(position); GLES20.glEnableVertexAttribArray(coord);
            GLES20.glUniformMatrix4fv(GLES20.glGetUniformLocation(program,"m"),1,false,matrix,0);
            GLES20.glUniform1i(GLES20.glGetUniformLocation(program,"s"),0);
            GLES20.glDrawArrays(GLES20.GL_TRIANGLE_STRIP,0,4);
            EGLExt.eglPresentationTimeANDROID(display,window,pts*1000);
            if(!EGL14.eglSwapBuffers(display,window)) throw new IllegalStateException("Surface swap failed");
        } catch(Throwable error) { failure=error; owner.stopped.set(true); }
    }
    void close() throws Exception {
        if(handler==null) return;
        CountDownLatch done=new CountDownLatch(1);
        handler.post(new Runnable() {
            @Override public void run() {
            try {
                if(surface!=null) surface.release();
                if(texture!=null) texture.release();
                if(display!=null && display!=EGL14.EGL_NO_DISPLAY) {
                    EGL14.eglMakeCurrent(display,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_CONTEXT);
                    if(window!=null) EGL14.eglDestroySurface(display,window);
                    if(context!=null) EGL14.eglDestroyContext(display,context);
                    EGL14.eglTerminate(display);
                }
            } finally { done.countDown(); thread.quitSafely(); }
            }
        });
        if(!done.await(2,TimeUnit.SECONDS)) throw new IllegalStateException("Surface cleanup pending");
        thread.join(2000);
        if(thread.isAlive()) throw new IllegalStateException("Surface worker cleanup pending");
    }
}
